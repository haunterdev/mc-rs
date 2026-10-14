//! java.util.Random, MathHelper, StrictMath.log, the noise generators and the biome table against
//! `misc.bin.gz` / `misc.json`.

mod common;

use common::*;
use worldgen::biome;
use worldgen::noise::{NoiseGeneratorOctaves, NoiseGeneratorPerlin, NoiseGeneratorSimplex};
use worldgen::{fdlibm, math_helper, JavaRandom};

fn misc() -> (Fixture, serde_json::Value) {
    (Fixture::load("misc.bin.gz"), json("misc.json"))
}

fn draw<T>(seed: i64, n: usize, mut f: impl FnMut(&mut JavaRandom) -> T) -> Vec<T> {
    let mut r = JavaRandom::new(seed);
    (0..n).map(|_| f(&mut r)).collect()
}

#[test]
fn java_random() {
    let (fx, js) = misc();
    let seeds: Vec<i64> = js["random_seeds"].as_array().unwrap().iter().map(|v| v.as_i64().unwrap()).collect();
    let bounds: Vec<i32> = js["random_bounds"].as_array().unwrap().iter().map(|v| v.as_i64().unwrap() as i32).collect();
    for (si, &s) in seeds.iter().enumerate() {
        let p = format!("random/{si}/");
        let l = |what: &str| format!("seed {s} {what}");
        assert_same(&l("nextInt"), fx.i32(&(p.clone() + "nextInt")), &draw(s, 64, |r| r.next_int()), flat);
        for &b in &bounds {
            assert_same(&l(&format!("nextInt({b})")), fx.i32(&format!("{p}nextInt/{b}")), &draw(s, 48, |r| r.next_int_bounded(b)), flat);
        }
        assert_same(&l("nextLong"), fx.i64(&(p.clone() + "nextLong")), &draw(s, 64, |r| r.next_long()), flat);
        assert_same(&l("nextDouble"), fx.f64(&(p.clone() + "nextDouble")), &draw(s, 64, |r| r.next_double()), flat);
        assert_same(&l("nextFloat"), fx.f32(&(p.clone() + "nextFloat")), &draw(s, 64, |r| r.next_float()), flat);
        assert_same(&l("nextGaussian"), fx.f64(&(p.clone() + "nextGaussian")), &draw(s, 64, |r| r.next_gaussian()), flat);
        assert_same(&l("nextBoolean"), fx.i32(&(p.clone() + "nextBoolean")), &draw(s, 64, |r| r.next_boolean() as i32), flat);

        let mut r = JavaRandom::new(s);
        let mut mix = Vec::with_capacity(80);
        for i in 0..16 {
            mix.push(r.next_gaussian().to_bits() as i64);
            mix.push(r.next_int_bounded(10) as i64);
            mix.push(r.next_gaussian().to_bits() as i64);
            mix.push(r.next_gaussian().to_bits() as i64);
            if i == 7 {
                r.set_seed(s);
            }
            mix.push(r.next_double().to_bits() as i64);
        }
        assert_same(&l("mixed calls"), fx.i64(&(p + "mixed")), &mix, flat);
    }
}

#[test]
fn math_helper() {
    let (fx, _) = misc();
    assert_same("SIN_TABLE", fx.f32("math/sin_table"), math_helper::sin_table(), flat);

    let fin = fx.f32("math/f_in");
    let at = |i: usize| format!("input {:?} (index {i})", fin[i]);
    let map = |f: fn(f32) -> f32| fin.iter().map(|&v| f(v)).collect::<Vec<_>>();
    assert_same("MathHelper.sin", fx.f32("math/sin"), &map(math_helper::sin), at);
    assert_same("MathHelper.cos", fx.f32("math/cos"), &map(math_helper::cos), at);
    assert_same("MathHelper.sqrt(float)", fx.f32("math/sqrt_f"), &map(math_helper::sqrt_f), at);

    let din = fx.f64("math/d_in");
    let at = |i: usize| format!("input {:?} (index {i})", din[i]);
    assert_same("StrictMath.log", fx.f64("math/strict_log"), &din.iter().map(|&v| fdlibm::log(v)).collect::<Vec<_>>(), at);
    assert_same("Math.round(double)", fx.i64("math/round"), &din.iter().map(|&v| math_helper::java_round(v)).collect::<Vec<_>>(), at);
    assert_same("MathHelper.floor", fx.i32("math/floor"), &din.iter().map(|&v| math_helper::floor(v)).collect::<Vec<_>>(), at);
    assert_same("MathHelper.lfloor", fx.i64("math/lfloor"), &din.iter().map(|&v| math_helper::lfloor(v)).collect::<Vec<_>>(), at);
}

/// Index into a 5x33x5 octave array (x outer, then z, then y).
fn xzy(i: usize) -> String {
    format!("x={} z={} y={}", i / (33 * 5), (i / 33) % 5, i % 33)
}

#[test]
fn noise() {
    let (fx, js) = misc();
    let seeds: Vec<i64> = js["noise_seeds"].as_array().unwrap().iter().map(|v| v.as_i64().unwrap()).collect();
    let pos: Vec<(i32, i32)> = js["noise_pos"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| (p[0].as_i64().unwrap() as i32, p[1].as_i64().unwrap() as i32))
        .collect();
    let scales: Vec<[f64; 3]> = js["noise_scales_bits"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| [0, 1, 2].map(|k| f64::from_bits(s[k].as_i64().unwrap() as u64)))
        .collect();
    let octaves = [16usize, 8];
    for (si, &s) in seeds.iter().enumerate() {
        for c in 0..2 {
            let n = NoiseGeneratorOctaves::new(&mut JavaRandom::new(s), octaves[c]);
            for (pi, &(px, pz)) in pos.iter().enumerate() {
                let mut v = Vec::new();
                n.generate_noise_octaves(&mut v, px, 0, pz, 5, 33, 5, scales[c][0], scales[c][1], scales[c][2]);
                let label = format!("seed {s} octaves({}) 3D at ({px}, 0, {pz})", octaves[c]);
                assert_same(&label, fx.f64(&format!("noise/oct3d/{si}/{c}/{pi}")), &v, xzy);
            }
        }
        let n2 = NoiseGeneratorOctaves::new(&mut JavaRandom::new(s), 16);
        for (pi, &(px, pz)) in pos.iter().enumerate() {
            let mut v = Vec::new();
            n2.generate_noise_octaves_2d(&mut v, px, pz, 5, 5, 200.0, 200.0, 0.5);
            let label = format!("seed {s} octaves(16) 2D at ({px}, {pz})");
            assert_same(&label, fx.f64(&format!("noise/oct2d/{si}/{pi}")), &v, |i| format!("x={} z={}", i / 5, i % 5));
        }
        let perlin4 = NoiseGeneratorPerlin::new(&mut JavaRandom::new(s), 4);
        for (pi, &(px, pz)) in pos.iter().enumerate() {
            let mut v = Vec::new();
            perlin4.get_region(&mut v, (px * 4) as f64, (pz * 4) as f64, 16, 16, 0.0625, 0.0625, 1.0);
            let label = format!("seed {s} perlin(4) region at ({}, {})", px * 4, pz * 4);
            assert_same(&label, fx.f64(&format!("noise/perlin_region/{si}/{pi}")), &v, flat);
        }
        let perlin1 = NoiseGeneratorPerlin::new(&mut JavaRandom::new(s), 1);
        let simplex = NoiseGeneratorSimplex::new(&mut JavaRandom::new(s));
        let xs = fx.f64(&format!("noise/xz/{si}/x"));
        let zs = fx.f64(&format!("noise/xz/{si}/z"));
        let at = |i: usize| format!("({:?}, {:?})", xs[i], zs[i]);
        let v1: Vec<f64> = (0..256).map(|i| perlin1.get_value(xs[i], zs[i])).collect();
        let v4: Vec<f64> = (0..256).map(|i| perlin4.get_value(xs[i] * 0.01, zs[i] * 0.01)).collect();
        let vs: Vec<f64> = (0..256).map(|i| simplex.get_value(xs[i] * 0.1, zs[i] * 0.1)).collect();
        assert_same(&format!("seed {s} perlin(1).getValue"), fx.f64(&format!("noise/perlin1_value/{si}")), &v1, at);
        assert_same(&format!("seed {s} perlin(4).getValue (x0.01)"), fx.f64(&format!("noise/perlin4_value/{si}")), &v4, at);
        assert_same(&format!("seed {s} simplex.getValue (x0.1)"), fx.f64(&format!("noise/simplex_value/{si}")), &vs, at);
    }
}

#[test]
fn biome_table() {
    let js = json("misc.json");
    let expected = js["biomes"].as_array().unwrap();
    let ours: Vec<_> = biome::all().collect();
    let exp_ids: Vec<i64> = expected.iter().map(|b| b["id"].as_i64().unwrap()).collect();
    let our_ids: Vec<i64> = ours.iter().map(|b| b.id as i64).collect();
    assert_eq!(exp_ids, our_ids, "registered biome ids");
    for (e, b) in expected.iter().zip(&ours) {
        let id = b.id;
        let bits = |k: &str| e[k].as_i64().unwrap() as u32;
        assert_eq!(e["name"].as_str().unwrap(), format!("minecraft:{}", b.name), "biome {id} name");
        assert_eq!(e["class"].as_str().unwrap(), b.biome_class().simple_name(), "biome {id} class");
        assert_eq!(bits("base_height"), b.base_height.to_bits(), "biome {id} base_height");
        assert_eq!(bits("height_variation"), b.height_variation.to_bits(), "biome {id} height_variation");
        assert_eq!(bits("temperature"), b.temperature.to_bits(), "biome {id} temperature");
        assert_eq!(bits("rainfall"), b.rainfall.to_bits(), "biome {id} rainfall");
        assert_eq!(e["snowy"].as_bool().unwrap(), b.snowy, "biome {id} snowy");
        assert_eq!(e["temp_category"].as_str().unwrap(), b.temp_category().name(), "biome {id} temp category");
        assert_eq!(e["top_block"].as_i64().unwrap(), b.top_block as i64, "biome {id} topBlock");
        assert_eq!(e["filler_block"].as_i64().unwrap(), b.filler_block as i64, "biome {id} fillerBlock");
        assert_eq!(e["is_mutation"].as_bool().unwrap(), b.is_mutation(), "biome {id} isMutation");
        let base = e["base_biome"]
            .as_str()
            .map(|n| biome::all().find(|o| o.name == n).unwrap_or_else(|| panic!("no biome named {n}")).id);
        assert_eq!(base, b.base_biome, "biome {id} base biome");
        let m = e["mutation"].as_i64().unwrap();
        assert_eq!(if m < 0 { None } else { Some(m as i32) }, biome::mutation_for(id as i32), "biome {id} mutation");
    }
}
