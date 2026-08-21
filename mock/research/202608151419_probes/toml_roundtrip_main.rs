use std::collections::BTreeMap;
use serde::{Deserialize, Serialize};

// Blocker found on the first run: plain `u64` fails to serialize via
// `toml` 0.8 whenever the value exceeds i64::MAX (TOML integers are
// signed 64-bit; no unsigned 64-bit type exists in the spec). Dylib
// hashes and digests are full-range u64 and routinely have the high
// bit set. This mirrors the exact problem config.rs's `de_seed`
// already solves for `master_seed`: serialize as a hex STRING, not a
// raw integer, and parse it back on the way in.
mod hex_u64 {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(v: &u64, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&format!("{v:#018x}"))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
        let s = String::deserialize(d)?;
        let t = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")).unwrap_or(&s);
        u64::from_str_radix(t, 16).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct AxisPoint {
    value: i64,
    label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct DroppedArm {
    arm: String,
    reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct CellRecordIdentityProbe {
    manifest_key: String,
    title: String,
    runid: String,
    axes: BTreeMap<String, AxisPoint>,
    dep_spec: Option<String>,
    arms_dropped: Vec<DroppedArm>,
    #[serde(with = "hex_u64")]
    resolved_seed: u64,
    arm_dylib_hash: BTreeMap<String, HashField>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct HashField(#[serde(with = "hex_u64")] u64);

fn main() {
    let mut axes = BTreeMap::new();
    axes.insert("n".to_string(), AxisPoint { value: 130003, label: None });
    axes.insert(
        "nc".to_string(),
        AxisPoint { value: 8192, label: Some("small".to_string()) },
    );

    let mut hashes = BTreeMap::new();
    // deliberately high-bit-set, the exact case that broke the first run.
    hashes.insert("kernel".to_string(), HashField(0xDEAD_BEEF_CAFE_1234u64));
    hashes.insert("headroom".to_string(), HashField(0x0000_0000_0000_0007u64));

    let rec = CellRecordIdentityProbe {
        manifest_key: "warm-container-width-l1".to_string(),
        title: "Warm/Precise container rule".to_string(),
        runid: "1755500000-4242".to_string(),
        axes,
        dep_spec: None,
        arms_dropped: vec![DroppedArm {
            arm: "lanes-deferred".to_string(),
            reason: "structural: validate_output rejected seed 7".to_string(),
        }],
        resolved_seed: 0xFFFF_FFFF_FFFF_FFFFu64, // max u64, the hardest case
        arm_dylib_hash: hashes,
    };

    let text = toml::to_string_pretty(&rec).expect("serialize");
    println!("--- rendered ---\n{text}");

    let back: CellRecordIdentityProbe = toml::from_str(&text).expect("deserialize");
    assert_eq!(rec, back, "round trip must be exact, including full-range u64 via hex string");
    println!("round trip OK, including u64::MAX seed and high-bit-set hashes");

    // Negative control: missing required field refused.
    let bad = r#"
        title = "x"
        runid = "y"
        resolved_seed = "0x1"
        dep_spec = "z"
        arms_dropped = []

        [axes]
        [arm_dylib_hash]
    "#;
    let refused = toml::from_str::<CellRecordIdentityProbe>(bad);
    assert!(refused.is_err(), "missing manifest_key must be refused, got {:?}", refused);
    println!("negative control: missing required field correctly refused: {:?}", refused.unwrap_err());

    // Negative control: empty axes map deserializes (non-empty is MY rule).
    let empty_axes = r#"
        manifest_key = "x"
        title = "x"
        runid = "y"
        resolved_seed = "0x1"
        dep_spec = "z"
        arms_dropped = []

        [axes]
        [arm_dylib_hash]
    "#;
    let with_empty: CellRecordIdentityProbe = toml::from_str(empty_axes).expect("deserialize empty axes");
    assert!(with_empty.axes.is_empty());
    println!("negative control: empty axes map deserializes fine (non-empty is a design rule, not free)");

    println!("ALL CHECKS PASSED");
}
