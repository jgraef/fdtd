pub mod palette {
    // todo: ideally we want something flexible, which also supports `#RRGGBB[AA]`.
    // we can also use the `is_human_readable` from `serde::Serializer`.
    // named colors with [`named::from_str`][1] would also be nice.
    // [1]: https://docs.rs/palette/latest/palette/named/fn.from_str.html

    // format for now we'll use the as_array variant
    pub use palette::serde::as_array::*;
}
