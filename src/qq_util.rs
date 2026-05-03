pub fn hash33(qrsig: &str) -> String {
    let mut e: i64 = 0;
    for ch in qrsig.chars() {
        e = e.wrapping_add((e << 5).wrapping_add(ch as i64));
    }
    (e & 0x7fffffff).to_string()
}

pub fn get_gtk(skey: &str) -> String {
    let mut hash: i32 = 5381;
    for ch in skey.chars() {
        hash = hash.wrapping_add((hash << 5).wrapping_add(ch as i32));
    }
    (hash & 0x7fffffff).to_string()
}

pub fn uin_to_qq(p_uin: &str) -> String {
    p_uin
        .trim_start_matches('o')
        .trim_start_matches('0')
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_uin_to_qq() {
        assert_eq!(uin_to_qq("o000000123456"), "123456");
    }

    #[test]
    fn hash33_is_stable() {
        assert_eq!(hash33("abc"), "108966");
    }

    #[test]
    fn hash33_handles_long_qrsig_without_debug_overflow() {
        let qrsig = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789".repeat(8);
        assert!(!hash33(&qrsig).is_empty());
    }

    #[test]
    fn get_gtk_handles_long_skey_without_debug_overflow() {
        let skey = "@abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789".repeat(8);
        assert!(!get_gtk(&skey).is_empty());
    }
}
