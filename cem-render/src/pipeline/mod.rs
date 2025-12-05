use bitflags::bitflags;

pub mod clear;
pub mod mesh;

#[derive(Clone, Copy, Debug)]
pub struct DepthState {
    pub write_enable: bool,
    pub compare: wgpu::CompareFunction,
    pub bias: wgpu::DepthBiasState,
}

impl DepthState {
    pub fn new(write_enable: bool, compare: wgpu::CompareFunction) -> Self {
        Self {
            write_enable,
            compare,
            bias: Default::default(),
        }
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
    pub struct Stencil: u8 {
        const OUTLINE = 0b0000_0001;
        const ALL     = 0b1111_1111;
    }
}

impl From<Stencil> for u32 {
    fn from(value: Stencil) -> Self {
        value.bits().into()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct StencilTest {
    pub read_mask: Stencil,
    pub compare: wgpu::CompareFunction,
}
