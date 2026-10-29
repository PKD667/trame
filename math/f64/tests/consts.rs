//! Each constant's value is the one in the fdlibm comment (the hex words), so a mistyped decimal
//! digit is caught even when no test of the function would notice it.

use super::super::{exp, expm1, ln, pow};

fn word(h: u32, l: u32) -> u64 {
    (u64::from(h) << 32) | u64::from(l)
}

#[test]
fn constants_match_fdlibm_hex_words() {
    let pairs = [
        (exp::P1, word(0x3FC5_5555, 0x5555_553E)),
        (exp::P2, word(0xBF66_C16C, 0x16BE_BD93)),
        (exp::P3, word(0x3F11_566A, 0xAF25_DE2C)),
        (exp::P4, word(0xBEBB_BD41, 0xC5D2_6BF1)),
        (exp::P5, word(0x3E66_3769, 0x72BE_A4D0)),
        (exp::LN2HI[0], word(0x3fe6_2e42, 0xfee0_0000)),
        (exp::LN2LO[0], word(0x3dea_39ef, 0x3579_3c76)),
        (exp::O_THRESHOLD, word(0x4086_2E42, 0xFEFA_39EF)),
        (exp::U_THRESHOLD, word(0xc087_4910, 0xD52D_3051)),
        (ln::LG1, word(0x3FE5_5555, 0x5555_5593)),
        (ln::LG2, word(0x3FD9_9999, 0x9997_FA04)),
        (ln::LG3, word(0x3FD2_4924, 0x9422_9359)),
        (ln::LG4, word(0x3FCC_71C5, 0x1D8E_78AF)),
        (ln::LG5, word(0x3FC7_4664, 0x96CB_03DE)),
        (ln::LG6, word(0x3FC3_9A09, 0xD078_C69F)),
        (ln::LG7, word(0x3FC2_F112, 0xDF3E_5244)),
        (expm1::Q1, word(0xBFA1_1111, 0x1111_10F4)),
        (expm1::Q2, word(0x3F5A_01A0, 0x19FE_5585)),
        (expm1::Q3, word(0xBF14_CE19, 0x9EAA_DBB7)),
        (expm1::Q4, word(0x3ED0_CFCA, 0x86E6_5239)),
        (expm1::Q5, word(0xBE8A_FDB7, 0x6E09_C32D)),
        (pow::log2::L1, word(0x3FE3_3333, 0x3333_3303)),
        (pow::log2::L2, word(0x3FDB_6DB6, 0xDB6F_ABFF)),
        (pow::log2::L3, word(0x3FD5_5555, 0x518F_264D)),
        (pow::log2::L4, word(0x3FD1_7460, 0xA91D_4101)),
        (pow::log2::L5, word(0x3FCD_864A, 0x93C9_DB65)),
        (pow::log2::L6, word(0x3FCA_7E28, 0x4A45_4EEF)),
        (pow::log2::DP_H[1], word(0x3FE2_B803, 0x4000_0000)),
        (pow::log2::DP_L[1], word(0x3E4C_FDEB, 0x43CF_D006)),
        (pow::log2::CP, word(0x3FEE_C709, 0xDC3A_03FD)),
        (pow::log2::CP_H, word(0x3FEE_C709, 0xE000_0000)),
        (pow::log2::CP_L, word(0xBE3E_2FE0, 0x145B_01F5)),
        (pow::log2::IVLN2, word(0x3FF7_1547, 0x652B_82FE)),
        (pow::log2::IVLN2_H, word(0x3FF7_1547, 0x6000_0000)),
        (pow::log2::IVLN2_L, word(0x3E54_AE0B, 0xF85D_DF44)),
        (pow::exp2::LG2, word(0x3FE6_2E42, 0xFEFA_39EF)),
        (pow::exp2::LG2_H, word(0x3FE6_2E43, 0x0000_0000)),
        (pow::exp2::LG2_L, word(0xBE20_5C61, 0x0CA8_6C39)),
    ];
    for (i, (value, bits)) in pairs.iter().enumerate() {
        assert_eq!(value.to_bits(), *bits, "constant {i} differs from its fdlibm hex comment");
    }
}
