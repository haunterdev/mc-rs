//! The GenLayer graph (classes, seeds, wiring) and every layer's output over the fixture regions.

mod common;

use std::sync::Arc;

use common::*;
use worldgen::genlayer::{BiomeLayers, GenLayer};

#[test]
fn layer_graph_and_outputs() {
    for seed in seeds() {
        let js = json(&format!("seed_{seed}.json"));
        let fx = Fixture::load(js["file"].as_str().unwrap());
        let layers = BiomeLayers::new(seed);
        let walk = layers.walk();
        let expected = js["layers"].as_array().unwrap();
        let index_of = |l: &Arc<GenLayer>| walk.iter().position(|o| Arc::ptr_eq(o, l)).unwrap();

        let classes: Vec<&str> = walk.iter().map(|l| l.class_name()).collect();
        let exp_classes: Vec<&str> = expected.iter().map(|l| l["class"].as_str().unwrap()).collect();
        assert_eq!(exp_classes, classes, "seed {seed}: layer graph order");
        assert_eq!(index_of(&layers.gen_biomes) as i64, js["gen_biomes_layer"].as_i64().unwrap(), "seed {seed}: genBiomes");
        assert_eq!(index_of(&layers.biome_index) as i64, js["index_layer"].as_i64().unwrap(), "seed {seed}: biomeIndexLayer");

        let regions: Vec<[i64; 4]> = js["layer_regions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| [0, 1, 2, 3].map(|k| r[k].as_i64().unwrap()))
            .collect();
        for (i, (l, e)) in walk.iter().zip(expected).enumerate() {
            let name = format!("seed {seed}: layer {i} {}", l.class_name());
            assert_eq!(e["baseSeed"].as_i64().unwrap(), l.base_seed, "{name} baseSeed");
            assert_eq!(e["worldGenSeed"].as_i64().unwrap(), l.world_gen_seed, "{name} worldGenSeed");
            let children: Vec<i64> = l.children().into_iter().map(|c| index_of(c) as i64).collect();
            let exp_children: Vec<i64> = e["children"].as_array().unwrap().iter().map(|c| c.as_i64().unwrap()).collect();
            assert_eq!(exp_children, children, "{name} parents");
            for (r, &[x, z, w, h]) in regions.iter().enumerate() {
                let (x, z, w, h) = (x as i32, z as i32, w as usize, h as usize);
                let got = l.get_ints(x, z, w, h);
                let label = format!("{name} getInts({x}, {z}, {w}, {h})");
                assert_same(&label, fx.i32(&format!("layer/{i}/{r}")), &got, |n| {
                    format!("x={} z={}", x + (n % w) as i32, z + (n / w) as i32)
                });
            }
        }
    }
}
