pub mod scene;
pub mod serde;

use std::thread::JoinHandle;

#[macro_export]
macro_rules! lipsum {
    ($n:expr) => {{
        static TEXT: ::std::sync::OnceLock<String> = ::std::sync::OnceLock::new();
        TEXT.get_or_init(|| ::lipsum::lipsum($n)).as_str()
    }};
}

pub fn spawn_thread<F, R>(name: impl ToString, f: F) -> JoinHandle<R>
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    std::thread::Builder::new()
        .name(name.to_string())
        .spawn(f)
        .expect("std::thread::spawn failed")
}
