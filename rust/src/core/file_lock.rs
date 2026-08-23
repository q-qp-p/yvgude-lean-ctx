use std::io::{Error, ErrorKind};

pub(crate) fn is_contended(error: &Error) -> bool {
    error.kind() == ErrorKind::WouldBlock
        || error
            .raw_os_error()
            .zip(fs2::lock_contended_error().raw_os_error())
            .is_some_and(|(error_code, contended_code)| error_code == contended_code)
}

#[cfg(test)]
mod tests {
    use super::is_contended;
    use std::io::{Error, ErrorKind};

    #[test]
    fn recognizes_fs2_contention_sentinel() {
        assert!(is_contended(&fs2::lock_contended_error()));
    }

    #[test]
    fn rejects_unrelated_errors() {
        assert!(!is_contended(&Error::new(
            ErrorKind::PermissionDenied,
            "unrelated",
        )));
    }
}
