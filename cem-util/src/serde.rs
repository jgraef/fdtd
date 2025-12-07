use serde::ser::{
    Error as _,
    Impossible,
    Serialize,
    SerializeMap,
    SerializeStruct,
    Serializer,
};

#[derive(Debug)]
pub struct FlattenMapSerializer<'a, M> {
    serialize_map: &'a mut M,
}

impl<'a, M> FlattenMapSerializer<'a, M> {
    pub fn new(serialize_map: &'a mut M) -> Self {
        Self { serialize_map }
    }
}

macro_rules! flatten_map_impossible_error {
    ($kind:literal) => {
        Err(Self::Error::custom(concat!(
            "can only flatten structs and maps (got a ",
            $kind,
            ")"
        )))
    };
}

#[allow(unused_variables)]
impl<'a, M> Serializer for FlattenMapSerializer<'a, M>
where
    M: SerializeMap,
{
    type Ok = ();
    type Error = M::Error;
    type SerializeSeq = Impossible<(), M::Error>;
    type SerializeTuple = Impossible<(), M::Error>;
    type SerializeTupleStruct = Impossible<(), M::Error>;
    type SerializeTupleVariant = Impossible<(), M::Error>;
    type SerializeMap = FlattenMapInto<'a, M>;
    type SerializeStruct = FlattenMapInto<'a, M>;
    type SerializeStructVariant = Impossible<(), M::Error>;

    fn serialize_bool(self, v: bool) -> Result<Self::Ok, Self::Error> {
        flatten_map_impossible_error!("bool")
    }

    fn serialize_i8(self, v: i8) -> Result<Self::Ok, Self::Error> {
        flatten_map_impossible_error!("i8")
    }

    fn serialize_i16(self, v: i16) -> Result<Self::Ok, Self::Error> {
        flatten_map_impossible_error!("i16")
    }

    fn serialize_i32(self, v: i32) -> Result<Self::Ok, Self::Error> {
        flatten_map_impossible_error!("i32")
    }

    fn serialize_i64(self, v: i64) -> Result<Self::Ok, Self::Error> {
        flatten_map_impossible_error!("i64")
    }

    fn serialize_u8(self, v: u8) -> Result<Self::Ok, Self::Error> {
        flatten_map_impossible_error!("u8")
    }

    fn serialize_u16(self, v: u16) -> Result<Self::Ok, Self::Error> {
        flatten_map_impossible_error!("u16")
    }

    fn serialize_u32(self, v: u32) -> Result<Self::Ok, Self::Error> {
        flatten_map_impossible_error!("u32")
    }

    fn serialize_u64(self, v: u64) -> Result<Self::Ok, Self::Error> {
        flatten_map_impossible_error!("u64")
    }

    fn serialize_f32(self, v: f32) -> Result<Self::Ok, Self::Error> {
        flatten_map_impossible_error!("f32")
    }

    fn serialize_f64(self, v: f64) -> Result<Self::Ok, Self::Error> {
        flatten_map_impossible_error!("f64")
    }

    fn serialize_char(self, v: char) -> Result<Self::Ok, Self::Error> {
        flatten_map_impossible_error!("char")
    }

    fn serialize_str(self, v: &str) -> Result<Self::Ok, Self::Error> {
        flatten_map_impossible_error!("&str")
    }

    fn serialize_bytes(self, v: &[u8]) -> Result<Self::Ok, Self::Error> {
        flatten_map_impossible_error!("&[u8]")
    }

    fn serialize_none(self) -> Result<Self::Ok, Self::Error> {
        flatten_map_impossible_error!("None")
    }

    fn serialize_some<T>(self, value: &T) -> Result<Self::Ok, Self::Error>
    where
        T: ?Sized + Serialize,
    {
        flatten_map_impossible_error!("Some(T)")
    }

    fn serialize_unit(self) -> Result<Self::Ok, Self::Error> {
        flatten_map_impossible_error!("()")
    }

    fn serialize_unit_struct(self, name: &'static str) -> Result<Self::Ok, Self::Error> {
        flatten_map_impossible_error!("()")
    }

    fn serialize_unit_variant(
        self,
        name: &'static str,
        variant_index: u32,
        variant: &'static str,
    ) -> Result<Self::Ok, Self::Error> {
        flatten_map_impossible_error!("()")
    }

    fn serialize_newtype_struct<T>(
        self,
        name: &'static str,
        value: &T,
    ) -> Result<Self::Ok, Self::Error>
    where
        T: ?Sized + Serialize,
    {
        flatten_map_impossible_error!("newtype struct")
    }

    fn serialize_newtype_variant<T>(
        self,
        name: &'static str,
        variant_index: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<Self::Ok, Self::Error>
    where
        T: ?Sized + Serialize,
    {
        flatten_map_impossible_error!("newtype variant")
    }

    fn serialize_seq(self, len: Option<usize>) -> Result<Self::SerializeSeq, Self::Error> {
        flatten_map_impossible_error!("sequence")
    }

    fn serialize_tuple(self, len: usize) -> Result<Self::SerializeTuple, Self::Error> {
        flatten_map_impossible_error!("tuple")
    }

    fn serialize_tuple_struct(
        self,
        name: &'static str,
        len: usize,
    ) -> Result<Self::SerializeTupleStruct, Self::Error> {
        flatten_map_impossible_error!("tuple struct")
    }

    fn serialize_tuple_variant(
        self,
        name: &'static str,
        variant_index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<Self::SerializeTupleVariant, Self::Error> {
        flatten_map_impossible_error!("tuple variant")
    }

    fn serialize_map(self, len: Option<usize>) -> Result<Self::SerializeMap, Self::Error> {
        Ok(FlattenMapInto {
            map: self.serialize_map,
        })
    }

    fn serialize_struct(
        self,
        name: &'static str,
        len: usize,
    ) -> Result<Self::SerializeStruct, Self::Error> {
        Ok(FlattenMapInto {
            map: self.serialize_map,
        })
    }

    fn serialize_struct_variant(
        self,
        name: &'static str,
        variant_index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<Self::SerializeStructVariant, Self::Error> {
        flatten_map_impossible_error!("struct variant")
    }
}

#[derive(Debug)]
pub struct FlattenMapInto<'a, M> {
    map: &'a mut M,
}

impl<'a, M> SerializeMap for FlattenMapInto<'a, M>
where
    M: SerializeMap,
{
    type Ok = ();
    type Error = M::Error;

    fn serialize_key<T>(&mut self, key: &T) -> Result<(), Self::Error>
    where
        T: ?Sized + Serialize,
    {
        self.map.serialize_key(key)
    }

    fn serialize_value<T>(&mut self, value: &T) -> Result<(), Self::Error>
    where
        T: ?Sized + Serialize,
    {
        self.map.serialize_value(value)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }
}

impl<'a, M> SerializeStruct for FlattenMapInto<'a, M>
where
    M: SerializeMap,
{
    type Ok = ();
    type Error = M::Error;

    fn serialize_field<T>(&mut self, key: &'static str, value: &T) -> Result<(), Self::Error>
    where
        T: ?Sized + Serialize,
    {
        self.map.serialize_entry(key, value)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        todo!()
    }
}
