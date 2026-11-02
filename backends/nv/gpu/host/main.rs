//! Host harness for the one-warp prototype (src/main.rs, the device code). Nix builds this program;
//! the CUDA driver is found at run time, not at build time: `libcuda.so.1` is opened with dlopen, so
//! the build needs no CUDA toolkit. The PTX is the file cargo-oxide built from src/main.rs.
//!
//! Usage: nv-post-parallel-host PTX_FILE
//! stdout: the same lines as programs/post_parallel_pod.expected (the program's output).
//! stderr: one JSON line `{"device":...}` with the device identity, then the slot state.
//! Exit: 0 when the kernel ran and its output was copied back; 3 when no driver could be opened
//! (nothing measured); 4 on any CUDA error or a missing kernel.
//!
//! The struct layouts and the kernel parameter list are the device's: #[repr(C)] Input, Slot, Report
//! (16 bytes each), and seven parameters: inputs (ptr, len), salt, slots (ptr, len), reports (ptr, len).

use std::ffi::{CString, c_char, c_int, c_uint, c_void};

type CUresult = c_int;
type CUdevice = c_int;
type CUcontext = *mut c_void;
type CUmodule = *mut c_void;
type CUfunction = *mut c_void;
type CUstream = *mut c_void;
type CUdeviceptr = u64;

const CU_DEVICE_ATTRIBUTE_COMPUTE_CAPABILITY_MAJOR: c_int = 75;
const CU_DEVICE_ATTRIBUTE_COMPUTE_CAPABILITY_MINOR: c_int = 76;
const RTLD_NOW: c_int = 2;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Input {
    key: u32,
    value: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Slot {
    acc: u64,
    seen: [u32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Report {
    acc: u64,
    slot: u32,
    branch: u32,
}

unsafe extern "C" {
    fn dlopen(filename: *const c_char, flags: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    fn dlerror() -> *const c_char;
}

/// The driver entry points used here, resolved by name from libcuda.so.1.
struct Driver {
    init: unsafe extern "C" fn(c_uint) -> CUresult,
    driver_version: unsafe extern "C" fn(*mut c_int) -> CUresult,
    device_get: unsafe extern "C" fn(*mut CUdevice, c_int) -> CUresult,
    device_name: unsafe extern "C" fn(*mut c_char, c_int, CUdevice) -> CUresult,
    device_attr: unsafe extern "C" fn(*mut c_int, c_int, CUdevice) -> CUresult,
    device_total_mem: unsafe extern "C" fn(*mut usize, CUdevice) -> CUresult,
    ctx_create: unsafe extern "C" fn(*mut CUcontext, c_uint, CUdevice) -> CUresult,
    module_load_data: unsafe extern "C" fn(*mut CUmodule, *const c_void) -> CUresult,
    module_get_function: unsafe extern "C" fn(*mut CUfunction, CUmodule, *const c_char) -> CUresult,
    mem_alloc: unsafe extern "C" fn(*mut CUdeviceptr, usize) -> CUresult,
    memcpy_htod: unsafe extern "C" fn(CUdeviceptr, *const c_void, usize) -> CUresult,
    memcpy_dtoh: unsafe extern "C" fn(*mut c_void, CUdeviceptr, usize) -> CUresult,
    launch: unsafe extern "C" fn(
        CUfunction,
        c_uint,
        c_uint,
        c_uint,
        c_uint,
        c_uint,
        c_uint,
        c_uint,
        CUstream,
        *mut *mut c_void,
        *mut *mut c_void,
    ) -> CUresult,
    synchronize: unsafe extern "C" fn() -> CUresult,
    get_error_name: unsafe extern "C" fn(CUresult, *mut *const c_char) -> CUresult,
}

fn fail(code: i32, msg: &str) -> ! {
    eprintln!("nv-post-parallel-host: {msg}");
    std::process::exit(code);
}

/// Opens libcuda.so.1 and resolves the entry points. Exit 3 when the driver is not there.
fn open_driver() -> Driver {
    let lib = CString::new("libcuda.so.1").unwrap();
    let h = unsafe { dlopen(lib.as_ptr(), RTLD_NOW) };
    if h.is_null() {
        let e = unsafe { dlerror() };
        let e = if e.is_null() { String::from("?") } else { unsafe { std::ffi::CStr::from_ptr(e) }.to_string_lossy().into_owned() };
        fail(3, &format!("no CUDA driver library: {e}"));
    }
    macro_rules! sym {
        ($name:literal) => {{
            let s = CString::new($name).unwrap();
            let p = unsafe { dlsym(h, s.as_ptr()) };
            if p.is_null() {
                fail(4, &format!("driver has no symbol {}", $name));
            }
            unsafe { std::mem::transmute(p) }
        }};
    }
    Driver {
        init: sym!("cuInit"),
        driver_version: sym!("cuDriverGetVersion"),
        device_get: sym!("cuDeviceGet"),
        device_name: sym!("cuDeviceGetName"),
        device_attr: sym!("cuDeviceGetAttribute"),
        device_total_mem: sym!("cuDeviceTotalMem_v2"),
        ctx_create: sym!("cuCtxCreate_v2"),
        module_load_data: sym!("cuModuleLoadData"),
        module_get_function: sym!("cuModuleGetFunction"),
        mem_alloc: sym!("cuMemAlloc_v2"),
        memcpy_htod: sym!("cuMemcpyHtoD_v2"),
        memcpy_dtoh: sym!("cuMemcpyDtoH_v2"),
        launch: sym!("cuLaunchKernel"),
        synchronize: sym!("cuCtxSynchronize"),
        get_error_name: sym!("cuGetErrorName"),
    }
}

impl Driver {
    fn check(&self, r: CUresult, what: &str) {
        if r != 0 {
            let mut name: *const c_char = std::ptr::null();
            unsafe { (self.get_error_name)(r, &mut name) };
            let name = if name.is_null() { String::from("?") } else { unsafe { std::ffi::CStr::from_ptr(name) }.to_string_lossy().into_owned() };
            fail(4, &format!("{what}: CUDA error {r} ({name})"));
        }
    }
}

/// Copies a `Copy` slice to the device; returns its address.
fn upload<T: Copy>(d: &Driver, v: &[T]) -> CUdeviceptr {
    let bytes = std::mem::size_of_val(v);
    let mut p: CUdeviceptr = 0;
    d.check(unsafe { (d.mem_alloc)(&mut p, bytes.max(1)) }, "cuMemAlloc");
    d.check(unsafe { (d.memcpy_htod)(p, v.as_ptr() as *const c_void, bytes) }, "cuMemcpyHtoD");
    p
}

fn download<T: Copy + Default>(d: &Driver, p: CUdeviceptr, n: usize) -> Vec<T> {
    let mut v = vec![T::default(); n];
    d.check(unsafe { (d.memcpy_dtoh)(v.as_mut_ptr() as *mut c_void, p, n * std::mem::size_of::<T>()) }, "cuMemcpyDtoH");
    v
}

fn main() {
    let ptx_path = std::env::args().nth(1).unwrap_or_else(|| fail(2, "usage: nv-post-parallel-host PTX_FILE"));
    let ptx = std::fs::read(&ptx_path).unwrap_or_else(|e| fail(2, &format!("cannot read {ptx_path}: {e}")));
    let image = CString::new(ptx).unwrap_or_else(|_| fail(2, "PTX has a NUL byte"));

    let d = open_driver();
    d.check(unsafe { (d.init)(0) }, "cuInit");
    let mut dv: c_int = 0;
    d.check(unsafe { (d.driver_version)(&mut dv) }, "cuDriverGetVersion");
    let mut dev: CUdevice = 0;
    d.check(unsafe { (d.device_get)(&mut dev, 0) }, "cuDeviceGet");
    let mut name = [0 as c_char; 256];
    d.check(unsafe { (d.device_name)(name.as_mut_ptr(), name.len() as c_int, dev) }, "cuDeviceGetName");
    let name = unsafe { std::ffi::CStr::from_ptr(name.as_ptr()) }.to_string_lossy().into_owned();
    let (mut cc_major, mut cc_minor, mut mem): (c_int, c_int, usize) = (0, 0, 0);
    d.check(unsafe { (d.device_attr)(&mut cc_major, CU_DEVICE_ATTRIBUTE_COMPUTE_CAPABILITY_MAJOR, dev) }, "cc major");
    d.check(unsafe { (d.device_attr)(&mut cc_minor, CU_DEVICE_ATTRIBUTE_COMPUTE_CAPABILITY_MINOR, dev) }, "cc minor");
    d.check(unsafe { (d.device_total_mem)(&mut mem, dev) }, "cuDeviceTotalMem");
    eprintln!(
        "{{\"device\":\"{}\",\"compute_capability\":\"{}.{}\",\"total_mem_bytes\":{},\"driver_version\":{},\"ptx_bytes\":{}}}",
        name.replace('"', "'"),
        cc_major,
        cc_minor,
        mem,
        dv,
        image.as_bytes().len()
    );

    let mut ctx: CUcontext = std::ptr::null_mut();
    d.check(unsafe { (d.ctx_create)(&mut ctx, 0, dev) }, "cuCtxCreate");
    let mut module: CUmodule = std::ptr::null_mut();
    d.check(unsafe { (d.module_load_data)(&mut module, image.as_ptr() as *const c_void) }, "cuModuleLoadData");
    let mut f: CUfunction = std::ptr::null_mut();
    let kname = CString::new("post_parallel").unwrap();
    d.check(unsafe { (d.module_get_function)(&mut f, module, kname.as_ptr()) }, "cuModuleGetFunction(post_parallel)");

    // The inputs and the salt of programs/post_parallel_pod.rs, in the same order.
    let inputs: Vec<Input> = (0..8).map(|i| Input { key: (i % 4) as u32, value: 3 + 7 * i as u64 }).collect();
    let salt: u64 = 5;
    let slots: Vec<Slot> = vec![Slot::default(); 4];
    let reports: Vec<Report> = vec![Report::default(); 4];

    let (in_p, slots_p, reports_p) = (upload(&d, &inputs), upload(&d, &slots), upload(&d, &reports));
    // Seven kernel parameters, in the device's order; each slice is (pointer, length).
    let (mut a0, mut a1, mut a2, mut a3, mut a4, mut a5, mut a6): (u64, u64, u64, u64, u64, u64, u64) =
        (in_p, inputs.len() as u64, salt, slots_p, slots.len() as u64, reports_p, reports.len() as u64);
    let mut params: [*mut c_void; 7] = [
        &mut a0 as *mut _ as *mut c_void,
        &mut a1 as *mut _ as *mut c_void,
        &mut a2 as *mut _ as *mut c_void,
        &mut a3 as *mut _ as *mut c_void,
        &mut a4 as *mut _ as *mut c_void,
        &mut a5 as *mut _ as *mut c_void,
        &mut a6 as *mut _ as *mut c_void,
    ];
    // One block of one warp: one logical worker.
    d.check(
        unsafe { (d.launch)(f, 1, 1, 1, 32, 1, 1, 0, std::ptr::null_mut(), params.as_mut_ptr(), std::ptr::null_mut()) },
        "cuLaunchKernel",
    );
    d.check(unsafe { (d.synchronize)() }, "cuCtxSynchronize");

    let slots_after: Vec<Slot> = download(&d, slots_p, slots.len());
    let reports_after: Vec<Report> = download(&d, reports_p, reports.len());
    eprintln!("slots after the warp: {:?}", slots_after.iter().map(|s| (s.acc, s.seen)).collect::<Vec<_>>());
    for r in &reports_after {
        println!("slot {} acc {} branch {}", r.slot, r.acc, r.branch);
    }
    println!("outcomes: [Returned]");
}
