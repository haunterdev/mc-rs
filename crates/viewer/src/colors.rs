//! Biome tints: the `BlockColors` handlers for the blocks the generator places (grass block, water,
//! lily pad), the grass colormap from the client jar, and `BiomeColorHelper`'s 3x3 average.

use std::io;

use worldgen::biome::{self, Biome};
use worldgen::blocks::id;
use worldgen::noise::perlin::NoiseGeneratorPerlin;
use worldgen::JavaRandom;

use crate::jar::Jar;
use crate::region::Region;
use crate::textures::decode_png;

pub struct Colors {
    /// `ColorizerGrass.grassBuffer`: `textures/colormap/grass.png` as ARGB.
    grass: Vec<u32>,
    /// `ColorizerFoliage.foliageBuffer`: `textures/colormap/foliage.png` as ARGB.
    foliage: Vec<u32>,
    /// `Biome.TEMPERATURE_NOISE`
    temperature_noise: NoiseGeneratorPerlin,
    /// `Biome.GRASS_COLOR_NOISE`
    grass_noise: NoiseGeneratorPerlin,
}

impl Colors {
    /// `GrassColorReloadListener` and `FoliageColorReloadListener.onResourceManagerReload`
    pub fn load(jar: &Jar) -> io::Result<Colors> {
        let map = |name: &str| -> io::Result<Vec<u32>> {
            let img = decode_png(&jar.asset(&format!("textures/colormap/{name}.png"))?)?;
            if img.pixels.len() != 65536 {
                return Err(io::Error::new(io::ErrorKind::InvalidData, format!("{name} colormap is not 256x256")));
            }
            Ok(img.pixels)
        };
        Ok(Colors::from_buffers(map("grass")?, map("foliage")?))
    }

    pub fn from_buffers(grass: Vec<u32>, foliage: Vec<u32>) -> Colors {
        Colors {
            grass,
            foliage,
            temperature_noise: NoiseGeneratorPerlin::new(&mut JavaRandom::new(1234), 1),
            grass_noise: NoiseGeneratorPerlin::new(&mut JavaRandom::new(2345), 1),
        }
    }

    /// `ColorizerGrass.getGrassColor`
    pub fn grass_colorizer(&self, temperature: f64, humidity: f64) -> i32 {
        let humidity = humidity * temperature;
        let i = ((1.0 - temperature) * 255.0) as i32;
        let j = ((1.0 - humidity) * 255.0) as i32;
        let k = j << 8 | i;
        if k > self.grass.len() as i32 {
            -65281
        } else {
            self.grass[k as usize] as i32
        }
    }

    /// `ColorizerFoliage.getFoliageColor`; vanilla has no bounds check here (clamped inputs keep
    /// the index inside the map).
    pub fn foliage_colorizer(&self, temperature: f64, humidity: f64) -> i32 {
        let humidity = humidity * temperature;
        let i = ((1.0 - temperature) * 255.0) as i32;
        let j = ((1.0 - humidity) * 255.0) as i32;
        self.foliage[(j << 8 | i) as usize] as i32
    }

    /// `Biome.getFoliageColorAtPos` with the `BiomeSwamp` and `BiomeMesa` overrides.
    pub fn biome_foliage(&self, b: &Biome, x: i32, y: i32, z: i32) -> i32 {
        use worldgen::biome::Kind;
        match b.kind {
            Kind::Swamp => 6975545,
            Kind::Mesa { .. } => 10387789,
            _ => {
                let d0 = self.temperature(b, x, y, z).clamp(0.0, 1.0) as f64;
                let d1 = b.rainfall.clamp(0.0, 1.0) as f64;
                self.foliage_colorizer(d0, d1)
            }
        }
    }

    /// `Biome.getTemperature(BlockPos)`
    pub fn temperature(&self, b: &Biome, x: i32, y: i32, z: i32) -> f32 {
        if y > 64 {
            let f = (self.temperature_noise.get_value((x as f32 / 8.0) as f64, (z as f32 / 8.0) as f64) * 4.0) as f32;
            b.temperature - (f + y as f32 - 64.0) * 0.05 / 30.0
        } else {
            b.temperature
        }
    }

    /// `Biome.getGrassColorAtPos` with the `BiomeSwamp`, `BiomeMesa` and `BiomeForest` (roofed)
    /// overrides.
    pub fn biome_grass(&self, b: &Biome, x: i32, y: i32, z: i32) -> i32 {
        use worldgen::biome::Kind;
        match b.kind {
            Kind::Swamp => {
                let d0 = self.grass_noise.get_value(x as f64 * 0.0225, z as f64 * 0.0225);
                if d0 < -0.1 {
                    5011004
                } else {
                    6975545
                }
            }
            Kind::Mesa { .. } => 9470285,
            _ => {
                let d0 = self.temperature(b, x, y, z).clamp(0.0, 1.0) as f64;
                let d1 = b.rainfall.clamp(0.0, 1.0) as f64;
                let i = self.grass_colorizer(d0, d1);
                // BiomeForest.Type.ROOFED: roofed_forest and mutated_roofed_forest.
                if b.id == 29 || b.id == 157 {
                    ((i & 16711422) + 2634762) >> 1
                } else {
                    i
                }
            }
        }
    }

    /// `BiomeColorHelper.getColorAtPos`: the per-channel mean over the 3x3 columns at `y`.
    fn average(r: &Region, x: i32, y: i32, z: i32, f: impl Fn(&Biome, i32, i32, i32) -> i32) -> i32 {
        let (ox, oz) = r.origin;
        let (mut i, mut j, mut k) = (0, 0, 0);
        // BlockPos.getAllInBoxMutable: x fastest, then y, then z.
        for dz in -1..=1 {
            for dx in -1..=1 {
                let b = biome_of(r.biome(x + dx, z + dz));
                let l = f(b, ox + x + dx, y, oz + z + dz);
                i += (l & 16711680) >> 16;
                j += (l & 65280) >> 8;
                k += l & 255;
            }
        }
        (i / 9 & 255) << 16 | (j / 9 & 255) << 8 | k / 9 & 255
    }

    /// `BiomeColorHelper.getGrassColorAtPos`, center-chunk-relative coordinates.
    pub fn grass_at(&self, r: &Region, x: i32, y: i32, z: i32) -> i32 {
        Self::average(r, x, y, z, |b, wx, wy, wz| self.biome_grass(b, wx, wy, wz))
    }

    /// `BiomeColorHelper.getFoliageColorAtPos`
    pub fn foliage_at(&self, r: &Region, x: i32, y: i32, z: i32) -> i32 {
        Self::average(r, x, y, z, |b, wx, wy, wz| self.biome_foliage(b, wx, wy, wz))
    }

    /// `BiomeColorHelper.getWaterColorAtPos`
    pub fn water_at(&self, r: &Region, x: i32, y: i32, z: i32) -> i32 {
        Self::average(r, x, y, z, |b, _, _, _| water_color(b))
    }

    /// `BlockColors.colorMultiplier` for an in-world block's actual state; -1 for blocks with no
    /// handler. A double plant's upper half carries its variant (see `blockstates`).
    pub fn multiplier(&self, state: u16, r: &Region, x: i32, y: i32, z: i32) -> i32 {
        match state >> 4 {
            id::GRASS | id::TALLGRASS | id::REEDS => self.grass_at(r, x, y, z),
            id::WATER | id::FLOWING_WATER => self.water_at(r, x, y, z),
            id::WATERLILY => 2129968,
            // BlockOldLeaf.VARIANT: spruce and birch keep fixed colors.
            id::LEAVES => match state & 3 {
                1 => 6396257,
                2 => 8431445,
                _ => self.foliage_at(r, x, y, z),
            },
            id::LEAVES2 | id::VINE => self.foliage_at(r, x, y, z),
            // EnumPlantType GRASS (2) and FERN (3), read at the lower half.
            id::DOUBLE_PLANT => match state & 7 {
                2 | 3 => self.grass_at(r, x, if state & 8 != 0 { y - 1 } else { y }, z),
                _ => -1,
            },
            _ => -1,
        }
    }
}

/// `Biome.getWaterColor`: 14745518 for the two swamps (`setWaterColor`), white for the rest.
pub fn water_color(b: &Biome) -> i32 {
    if matches!(b.kind, worldgen::biome::Kind::Swamp) {
        14745518
    } else {
        16777215
    }
}

/// `Biome.getBiome(id)`; generated biome arrays only hold registered ids.
pub fn biome_of(id: u8) -> &'static Biome {
    biome::get(id as i32).unwrap_or_else(|| biome::get(1).expect("plains"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plains_grass_is_the_familiar_green() {
        let Ok(jar) = Jar::find() else {
            eprintln!("skipped: no client jar");
            return;
        };
        let c = Colors::load(&jar).unwrap();
        let plains = biome_of(1);
        // Plains: temperature 0.8, rainfall 0.4, below y 65 no noise.
        assert_eq!(c.biome_grass(plains, 0, 64, 0) & 0xFFFFFF, 0x91BD59);
        let swamp = biome_of(6);
        let s = c.biome_grass(swamp, 100, 64, 100);
        assert!(s == 5011004 || s == 6975545);
        assert_eq!(c.biome_grass(biome_of(37), 0, 64, 0), 9470285);
    }

    #[test]
    fn colorizer_indexes_temperature_then_humidity() {
        let mut buf = vec![0u32; 65536];
        // temperature 0.8, humidity 0.4 * 0.8 = 0.32 -> i = 50, j = 173.
        buf[173 << 8 | 50] = 0xFF123456;
        let c = Colors::from_buffers(buf.clone(), buf);
        assert_eq!(c.grass_colorizer(0.8, 0.4) as u32, 0xFF123456);
        assert_eq!(c.foliage_colorizer(0.8, 0.4) as u32, 0xFF123456);
    }

    #[test]
    fn leaves_take_foliage_except_spruce_and_birch() {
        use crate::region::tests::{chunk, region_of};
        use worldgen::blocks::{id, state};
        let c = Colors::from_buffers(vec![0xFF00FF00; 65536], vec![0xFF0000FF; 65536]);
        let r = region_of(chunk(0, 0, 64, |_| ()), 64);
        assert_eq!(c.multiplier(state(id::LEAVES, 0), &r, 3, 70, 3), 0x0000FF);
        assert_eq!(c.multiplier(state(id::LEAVES, 9), &r, 3, 70, 3), 6396257);
        assert_eq!(c.multiplier(state(id::LEAVES2, 1), &r, 3, 70, 3), 0x0000FF);
        assert_eq!(c.multiplier(state(id::TALLGRASS, 1), &r, 3, 64, 3), 0x00FF00);
        assert_eq!(c.multiplier(state(id::DOUBLE_PLANT, 8 | 2), &r, 3, 65, 3), 0x00FF00);
        assert_eq!(c.multiplier(state(id::DOUBLE_PLANT, 8), &r, 3, 65, 3), -1);
    }

    #[test]
    fn high_ground_is_colder() {
        let c = Colors::from_buffers(vec![0; 65536], vec![0; 65536]);
        let plains = biome_of(1);
        assert_eq!(c.temperature(plains, 0, 64, 0), 0.8);
        assert!(c.temperature(plains, 0, 200, 0) < 0.6);
    }
}
