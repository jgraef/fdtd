use nalgebra::{
    Point3,
    Vector3,
};

#[derive(Clone, Debug)]
pub struct Lattice<T> {
    dimensions: Vector3<usize>,
    encoder: Strider,
    data: Box<[T]>,
}

impl<T> Lattice<T> {
    pub fn new(dimensions: Vector3<usize>, mut init: impl FnMut(Point3<usize>) -> T) -> Self {
        let encoder = Strider::from_dimensions(&dimensions);
        let size = dimensions.product();

        let mut data = Box::new_uninit_slice(size);

        for x in 0..dimensions.x {
            for y in 0..dimensions.y {
                for z in 0..dimensions.z {
                    let point = Point3::new(x, y, z);
                    let index = encoder.to_index(&point);
                    data[index].write(init(point));
                }
            }
        }
        let data = unsafe { data.assume_init() };

        Self {
            dimensions,
            encoder,
            data,
        }
    }

    pub fn dimensions(&self) -> Vector3<usize> {
        self.dimensions
    }

    fn check_if_point_is_inside(&self, point: &Point3<usize>) -> bool {
        point.x < self.dimensions.x && point.y < self.dimensions.y && point.z < self.dimensions.z
    }

    pub fn get(&self, point: &Point3<usize>) -> Option<&T> {
        if self.check_if_point_is_inside(point) {
            let index = self.encoder.to_index(point);
            Some(&self.data[index])
        }
        else {
            None
        }
    }

    pub fn get_mut(&mut self, point: &Point3<usize>) -> Option<&mut T> {
        if self.check_if_point_is_inside(point) {
            let index = self.encoder.to_index(point);
            Some(&mut self.data[index])
        }
        else {
            None
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = (Point3<usize>, &T)> {
        self.data.iter().enumerate().map(|(index, data)| {
            let point = self.encoder.from_index(index);
            (point, data)
        })
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (Point3<usize>, &mut T)> {
        self.data.iter_mut().enumerate().map(|(index, data)| {
            let point = self.encoder.from_index(index);
            (point, data)
        })
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Strider {
    strides: Vector3<usize>,
}

impl Strider {
    pub fn new(strides: Vector3<usize>) -> Self {
        Self { strides }
    }

    pub fn from_dimensions(dimensions: &Vector3<usize>) -> Self {
        let strides = Vector3::new(1, dimensions.x, dimensions.x * dimensions.y);
        Self::new(strides)
    }

    pub fn from_index(&self, mut index: usize) -> Point3<usize> {
        let z = index / self.strides.z;
        index %= self.strides.z;
        let y = index / self.strides.y;
        index %= self.strides.y;
        let x = index / self.strides.x;
        Point3::new(x, y, z)
    }

    pub fn to_index(&self, point: &Point3<usize>) -> usize {
        point.coords.dot(&self.strides)
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct MortonEncoder;

impl MortonEncoder {
    #[inline(always)]
    pub fn from_index(&self, index: usize) -> Point3<usize> {
        let point: [u16; 3] = morton_encoding::morton_decode(index as u64);
        Point3::new(point[0] as usize, point[1] as usize, point[2] as usize)
    }

    #[inline(always)]
    pub fn to_index(&self, position: &Point3<usize>) -> usize {
        let index = morton_encoding::morton_encode([
            position.x as u16,
            position.y as u16,
            position.z as u16,
        ]);
        index as usize
    }
}
