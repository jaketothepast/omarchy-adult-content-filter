use std::{fs, io::Cursor, process::Command};

use image::{DynamicImage, ImageFormat, Rgb, RgbImage};

// Production mutation caught: extra stdout records or fields can disclose the input path/body,
// while missing model execution, dimensions, or timings violates the standalone infer contract.
#[test]
fn infer_emits_one_safe_json_record_for_a_generated_png() {
    let image = RgbImage::from_fn(2, 1, |x, _| match x {
        0 => Rgb([10, 20, 30]),
        1 => Rgb([40, 50, 60]),
        _ => unreachable!(),
    });
    let mut encoded = Cursor::new(Vec::new());
    DynamicImage::ImageRgb8(image)
        .write_to(&mut encoded, ImageFormat::Png)
        .unwrap();
    let temporary_directory = tempfile::tempdir().unwrap();
    let input_path = temporary_directory.path().join("sensitive-local-input.png");
    fs::write(&input_path, encoded.into_inner()).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_omarchy-kids-browser-filter"))
        .arg("infer")
        .arg(&input_path)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "infer failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(stdout.matches('\n').count(), 1);
    assert!(!stdout.contains("sensitive-local-input.png"));
    let record: serde_json::Value = serde_json::from_str(stdout.trim_end()).unwrap();
    let object = record.as_object().unwrap();
    let mut keys: Vec<&str> = object.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "decode_micros",
            "detections",
            "encoded_bytes",
            "height",
            "inference_micros",
            "model_sha256",
            "postprocess_micros",
            "preprocess_micros",
            "width",
        ]
    );
    assert_eq!(
        record["model_sha256"],
        "c15d8273adad2d0a92f014cc69ab2d6c311a06777a55545f2c4eb46f51911f0f"
    );
    assert_eq!(record["encoded_bytes"], 75);
    assert_eq!(record["width"], 2);
    assert_eq!(record["height"], 1);
    assert!(record["decode_micros"].is_u64());
    assert!(record["preprocess_micros"].is_u64());
    assert!(record["inference_micros"].is_u64());
    assert!(record["postprocess_micros"].is_u64());
    assert!(record["detections"].is_array());
}
