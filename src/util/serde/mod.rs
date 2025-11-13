pub mod palette {
    // todo: ideally we want something flexible, which also supports `#RRGGBB[AA]`.
    // we can also use the `is_human_readable` from `serde::Serializer`.

    // format for now we'll use the as_array variant
    pub use palette::serde::as_array::*;
}
