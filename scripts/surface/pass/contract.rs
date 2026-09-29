use trame::{Addr, Channel, Context, Deployment, Edge, Environment, Failure, Frame, Handle, Io, Launch, Reading, Shared, Tag};

struct SendOwner(u32);
fn send<T: Send>() {}
fn sync<T: Sync>() {}

pub fn client(cx: &mut Context, io: &mut Io<'_>, leader: &mut trame::leader::Leader) {
    send::<Context>();
    send::<Io<'_>>();
    sync::<trame::sync::Exclusive<SendOwner>>();
    let hosts: &[&[Launch]] = &[];
    let _ = Deployment::new(hosts, 0, Launch::new(0));
    let _ = Tag::new(0).get();
    let _ = Launch::new(0).get();
    let _ = Environment::default();
    let _: fn(Environment, Deployment) -> Result<Context, Failure> = trame::init;
    let _: fn(&Context) -> u32 = trame::rank;
    let _: fn(&Context) -> u32 = trame::size;
    let _: fn(&mut Context, Result<(), Failure>) -> Result<(), Failure> = trame::done::<()>;
    let _: fn(Environment, Deployment) -> Result<trame::leader::Leader, Failure> = trame::leader::open;
    let _ = trame::rank(cx);
    let _ = trame::size(cx);
    let _ = trame::done::<()>(cx, Ok(()));
    let _ = trame::send(cx, Addr::Local(0), Channel::Message(Tag::new(0)), &[]);
    let _ = trame::recv(cx, &mut []);
    let _ = trame::flush(cx);
    let _: fn(&trame::leader::Leader, u32, Tag, &[u8]) -> Result<(), trame::Error> = trame::leader::send_to;
    let _: fn(&trame::leader::Leader, &mut [u8]) -> Result<Option<Frame>, trame::Error> = trame::leader::recv_from;
    let _ = trame::leader::send_to(leader, 0, Tag::new(0), &[]);
    let _ = trame::leader::recv_from(leader, &mut []);
    let _ = trame::leader::done::<()>(leader, Ok(()));
    let _ = io.send(Addr::Local(0), Channel::Lane, &[]);
    let _ = io.lead(Tag::new(0), &[]);
    let _ = io.recv(&mut []);
    let _ = io.flush();
    let _ = Edge::new(Addr::Local(0), Addr::Local(0), std::num::NonZeroU32::new(1).unwrap());
    let _ = Handle::BYTES;
    let _ = Handle::to_bytes;
    let _ = Handle::from_bytes;
    let _ = trame::leader::publish;
    let _ = trame::leader::handle;
    let _ = trame::leader::retire;
    let _ = trame::attach;
    let _ = trame::bytes;
    let _ = trame::detach;
    let _ = trame::sync::with::<String, String>;
    let _ = trame::sync::handoff::new::<String, 1>;
    let _ = trame::sync::atomic::AtomicU32::new(0);
    let _ = trame::barrier;
    let _ = trame::reshape;
    let _ = trame::release;
    let _ = trame::clock::reading;
    let _: Option<Frame> = None;
    let _: Option<Shared> = None;
    let _: Option<Reading> = None;
    let _ = Frame::source;
    let _ = Frame::tag;
    let _ = Frame::len;
    let _ = Reading::since;
    let _ = trame::sync::atomic::AtomicBool::new(false);
    let _ = trame::sync::atomic::AtomicU64::new(0);
    let _: Option<Failure<()>> = None;
    let _: trame::Backend = trame::ID;
    let _: bool = trame::LOSSY;
    let _: usize = trame::MAX_FRAME;
}

const _: () = assert!(trame::MAX_FRAME >= 65_544 && trame::MAX_FRAME <= u32::MAX as usize);

#[trame::process]
struct Work;
impl Work {
    fn step(&mut self) -> Result<trame::Step, ()> { Ok(trame::Step::Done) }
}
pub fn process_witness() {
    let work = Work;
    let _ = trame::concurrent!(work);
}

#[derive(Clone, Copy)]
pub struct Item(pub u32);
pub struct SharedContext;
pub struct NonCopyError(String);
struct Client;
impl Client {
    #[trame::parallel]
    fn visit(&self, _: Item, _: &SharedContext) -> Result<(), NonCopyError> {
        Err(NonCopyError(String::new()))
    }

    #[trame::parallel]
    #[trame::ordered(key = item.0: usize)]
    fn update(&self, item: Item, state: &mut SendOwner, _: &SharedContext) -> Result<(), NonCopyError> {
        state.0 += item.0;
        Ok(())
    }
}

pub fn invoke_witness(items: &[Item], states: &mut [SendOwner], context: &SharedContext) -> Result<(), trame::Invoked<NonCopyError>> {
    let client = Client;
    trame::invoke!(client.visit, context, items)?;
    trame::invoke!(client.update, context, items, trame::Keyed::new(states))?;
    Ok(())
}

pub fn ownership_witness() {
    let exclusive = trame::sync::Exclusive::new(String::new());
    let _: Result<String, _> = trame::sync::with(&exclusive, |value| std::mem::replace(value, String::new()));
    let mut handoff = trame::sync::handoff::new::<String, 1>(String::new);
    let _ = trame::sync::handoff::split(&mut handoff);
}
