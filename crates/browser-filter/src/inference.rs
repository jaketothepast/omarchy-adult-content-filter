use std::{
    fs::File,
    io::{Cursor, Read},
    path::PathBuf,
    sync::OnceLock,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use image::{ImageReader, Limits, RgbImage, imageops};
use ort::{
    session::{Session, builder::GraphOptimizationLevel},
    value::TensorRef,
};
use serde::Serialize;
use sha2::{Digest, Sha256};

const MODEL_SIZE: u32 = 320;
const OUTPUT_CHANNELS: usize = 22;
const OUTPUT_CANDIDATES: usize = 2100;
const CANDIDATE_SCORE_THRESHOLD: f32 = 0.20;
const FINAL_SCORE_THRESHOLD: f32 = 0.25;
const NMS_IOU_THRESHOLD: f32 = 0.45;
pub const DEFAULT_MAX_ENCODED_BYTES: usize = 16 * 1024 * 1024;
pub const DEFAULT_MAX_PIXELS: u64 = 40_000_000;
pub const MODEL_SHA256: &str = "c15d8273adad2d0a92f014cc69ab2d6c311a06777a55545f2c4eb46f51911f0f";
const MAX_MODEL_BYTES: u64 = 64 * 1024 * 1024;
const LABELS: [&str; 18] = [
    "FEMALE_GENITALIA_COVERED",
    "FACE_FEMALE",
    "BUTTOCKS_EXPOSED",
    "FEMALE_BREAST_EXPOSED",
    "FEMALE_GENITALIA_EXPOSED",
    "MALE_BREAST_EXPOSED",
    "ANUS_EXPOSED",
    "FEET_EXPOSED",
    "BELLY_COVERED",
    "FEET_COVERED",
    "ARMPITS_COVERED",
    "ARMPITS_EXPOSED",
    "FACE_MALE",
    "BELLY_EXPOSED",
    "MALE_GENITALIA_EXPOSED",
    "ANUS_COVERED",
    "FEMALE_BREAST_COVERED",
    "BUTTOCKS_COVERED",
];

#[derive(Clone, Copy, Debug)]
pub struct PreprocessLimits {
    pub max_encoded_bytes: usize,
    pub max_pixels: u64,
}

impl Default for PreprocessLimits {
    fn default() -> Self {
        Self {
            max_encoded_bytes: DEFAULT_MAX_ENCODED_BYTES,
            max_pixels: DEFAULT_MAX_PIXELS,
        }
    }
}

#[derive(Debug)]
pub struct PreparedImage {
    pub shape: [usize; 4],
    pub data: Vec<f32>,
    pub original_size: (u32, u32),
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Detection {
    #[serde(rename = "class")]
    pub class_name: &'static str,
    pub score: f32,
    #[serde(rename = "box")]
    pub bounding_box: [u32; 4],
}

struct Candidate {
    class_name: &'static str,
    score: f32,
    bounding_box: [f32; 4],
}

#[derive(Clone, Debug)]
pub struct ModelConfig {
    pub model_path: PathBuf,
    pub runtime_path: PathBuf,
    pub max_encoded_bytes: usize,
    pub max_pixels: u64,
}

pub struct Detector {
    session: Session,
    limits: PreprocessLimits,
    metadata: DetectorMetadata,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DetectorMetadata {
    pub model_sha256: String,
    pub runtime_path: PathBuf,
}

#[derive(Debug, Serialize)]
pub struct InferenceReport {
    pub detections: Vec<Detection>,
    pub encoded_bytes: usize,
    pub width: u32,
    pub height: u32,
    pub decode_micros: u64,
    pub preprocess_micros: u64,
    pub inference_micros: u64,
    pub postprocess_micros: u64,
}

struct TimedPreparedImage {
    image: PreparedImage,
    decode_micros: u64,
    preprocess_micros: u64,
}

static RUNTIME_INITIALIZATION: OnceLock<Result<PathBuf, String>> = OnceLock::new();

pub fn preprocess(encoded: &[u8], limits: PreprocessLimits) -> Result<PreparedImage> {
    Ok(preprocess_with_timings(encoded, limits)?.image)
}

fn preprocess_with_timings(encoded: &[u8], limits: PreprocessLimits) -> Result<TimedPreparedImage> {
    anyhow::ensure!(
        encoded.len() <= limits.max_encoded_bytes,
        "encoded image is {} bytes; limit is {} bytes",
        encoded.len(),
        limits.max_encoded_bytes
    );

    let decode_started = Instant::now();
    let max_dimension = u32::try_from(limits.max_pixels).unwrap_or(u32::MAX);
    let mut image_limits = Limits::default();
    image_limits.max_image_width = Some(max_dimension);
    image_limits.max_image_height = Some(max_dimension);
    image_limits.max_alloc = Some(limits.max_pixels.saturating_mul(8));
    let mut dimensions_reader = ImageReader::new(Cursor::new(encoded)).with_guessed_format()?;
    dimensions_reader.limits(image_limits.clone());
    let original_size = dimensions_reader.into_dimensions()?;
    let pixel_count = u64::from(original_size.0) * u64::from(original_size.1);
    anyhow::ensure!(
        pixel_count <= limits.max_pixels,
        "decoded image is {pixel_count} pixels; limit is {} pixels",
        limits.max_pixels
    );
    let square_size = original_size.0.max(original_size.1);
    let padded_pixel_count = u64::from(square_size) * u64::from(square_size);
    anyhow::ensure!(
        padded_pixel_count <= limits.max_pixels,
        "padded image is {padded_pixel_count} pixels; limit is {} pixels",
        limits.max_pixels
    );

    let mut reader = ImageReader::new(Cursor::new(encoded)).with_guessed_format()?;
    reader.limits(image_limits);
    let image = reader.decode()?.into_rgb8();
    let decode_micros = elapsed_micros(decode_started.elapsed());

    let preprocess_started = Instant::now();
    let mut square = RgbImage::new(square_size, square_size);
    imageops::replace(&mut square, &image, 0, 0);
    let resized = imageops::resize(
        &square,
        MODEL_SIZE,
        MODEL_SIZE,
        imageops::FilterType::Triangle,
    );

    let plane_size = (MODEL_SIZE * MODEL_SIZE) as usize;
    let mut data = vec![0.0; plane_size * 3];
    for (index, pixel) in resized.pixels().enumerate() {
        data[index] = f32::from(pixel[2]) / 255.0;
        data[plane_size + index] = f32::from(pixel[1]) / 255.0;
        data[2 * plane_size + index] = f32::from(pixel[0]) / 255.0;
    }

    Ok(TimedPreparedImage {
        image: PreparedImage {
            shape: [1, 3, MODEL_SIZE as usize, MODEL_SIZE as usize],
            data,
            original_size,
        },
        decode_micros,
        preprocess_micros: elapsed_micros(preprocess_started.elapsed()),
    })
}

impl Detector {
    pub fn load(config: ModelConfig) -> Result<Self> {
        let (model_bytes, model_sha256) = read_verified_model(&config.model_path)?;
        let runtime_path = config
            .runtime_path
            .canonicalize()
            .context("failed to resolve ONNX Runtime library path")?;
        initialize_runtime(&runtime_path)?;
        let session = Session::builder()?
            .with_optimization_level(GraphOptimizationLevel::All)
            .map_err(|error| anyhow::anyhow!(error.to_string()))?
            .commit_from_memory(&model_bytes)
            .context("failed to load NudeNet model")?;

        Ok(Self {
            session,
            limits: PreprocessLimits {
                max_encoded_bytes: config.max_encoded_bytes,
                max_pixels: config.max_pixels,
            },
            metadata: DetectorMetadata {
                model_sha256,
                runtime_path,
            },
        })
    }

    pub fn metadata(&self) -> &DetectorMetadata {
        &self.metadata
    }

    pub fn detect(&mut self, encoded: &[u8]) -> Result<InferenceReport> {
        let prepared = preprocess_with_timings(encoded, self.limits)?;
        let inference_started = Instant::now();
        let tensor =
            TensorRef::from_array_view((prepared.image.shape, prepared.image.data.as_slice()))?;
        let outputs = self.session.run(ort::inputs!["images" => tensor])?;
        let inference_micros = elapsed_micros(inference_started.elapsed());

        let postprocess_started = Instant::now();
        let output = outputs
            .get("output0")
            .context("model did not return output0")?;
        let (shape, values) = output.try_extract_tensor::<f32>()?;
        anyhow::ensure!(
            shape[..] == [1, OUTPUT_CHANNELS as i64, OUTPUT_CANDIDATES as i64],
            "model output shape is {shape}; expected [1, 22, 2100]"
        );
        let detections = decode_detections(values, prepared.image.original_size)?;
        let postprocess_micros = elapsed_micros(postprocess_started.elapsed());

        Ok(InferenceReport {
            detections,
            encoded_bytes: encoded.len(),
            width: prepared.image.original_size.0,
            height: prepared.image.original_size.1,
            decode_micros: prepared.decode_micros,
            preprocess_micros: prepared.preprocess_micros,
            inference_micros,
            postprocess_micros,
        })
    }
}

fn read_verified_model(model_path: &std::path::Path) -> Result<(Vec<u8>, String)> {
    let mut model = File::open(model_path).context("failed to open NudeNet model")?;
    let model_bytes = model
        .metadata()
        .context("failed to inspect NudeNet model")?
        .len();
    anyhow::ensure!(
        model_bytes <= MAX_MODEL_BYTES,
        "model is {model_bytes} bytes; limit is {MAX_MODEL_BYTES} bytes"
    );

    let mut hasher = Sha256::new();
    let mut verified_bytes = Vec::with_capacity(usize::try_from(model_bytes).unwrap_or(0));
    let mut buffer = [0; 64 * 1024];
    loop {
        let bytes_read = model
            .read(&mut buffer)
            .context("failed to hash NudeNet model")?;
        if bytes_read == 0 {
            break;
        }
        anyhow::ensure!(
            verified_bytes.len().saturating_add(bytes_read)
                <= usize::try_from(MAX_MODEL_BYTES).unwrap_or(usize::MAX),
            "model grew beyond the {MAX_MODEL_BYTES} byte limit while hashing"
        );
        hasher.update(&buffer[..bytes_read]);
        verified_bytes.extend_from_slice(&buffer[..bytes_read]);
    }
    let model_sha256 = format!("{:x}", hasher.finalize());
    anyhow::ensure!(
        model_sha256 == MODEL_SHA256,
        "model SHA-256 {model_sha256} does not match pinned {MODEL_SHA256}"
    );
    Ok((verified_bytes, model_sha256))
}

fn initialize_runtime(runtime_path: &std::path::Path) -> Result<()> {
    let initialization = RUNTIME_INITIALIZATION.get_or_init(|| {
        let builder = ort::init_from(runtime_path).map_err(|error| error.to_string())?;
        if !builder.commit() {
            return Err("ONNX Runtime was already initialized".to_owned());
        }
        Ok(runtime_path.to_path_buf())
    });
    match initialization {
        Ok(initialized_path) => {
            anyhow::ensure!(
                initialized_path == runtime_path,
                "ONNX Runtime was initialized from a different library"
            );
            Ok(())
        }
        Err(message) => anyhow::bail!(message.clone()),
    }
}

fn elapsed_micros(duration: Duration) -> u64 {
    u64::try_from(duration.as_micros()).unwrap_or(u64::MAX)
}

pub fn decode_detections(output: &[f32], original_size: (u32, u32)) -> Result<Vec<Detection>> {
    let mut candidates = decode_candidates(output, original_size)?;
    candidates.retain(|candidate| candidate.score > FINAL_SCORE_THRESHOLD);
    candidates.sort_by(|left, right| right.score.total_cmp(&left.score));
    let mut selected: Vec<Candidate> = Vec::new();
    for candidate in candidates {
        if selected.iter().all(|other| {
            intersection_over_union(&candidate.bounding_box, &other.bounding_box)
                <= NMS_IOU_THRESHOLD
        }) {
            selected.push(candidate);
        }
    }

    Ok(selected
        .into_iter()
        .map(|candidate| Detection {
            class_name: candidate.class_name,
            score: candidate.score,
            bounding_box: candidate.bounding_box.map(|coordinate| coordinate as u32),
        })
        .collect())
}

fn decode_candidates(output: &[f32], original_size: (u32, u32)) -> Result<Vec<Candidate>> {
    anyhow::ensure!(
        output.len() == OUTPUT_CHANNELS * OUTPUT_CANDIDATES,
        "model output has {} values; expected {}",
        output.len(),
        OUTPUT_CHANNELS * OUTPUT_CANDIDATES
    );

    let scale = original_size.0.max(original_size.1) as f32 / MODEL_SIZE as f32;
    let mut candidates = Vec::new();
    for candidate in 0..OUTPUT_CANDIDATES {
        let (class_id, score) = LABELS
            .iter()
            .enumerate()
            .map(|(class_id, _)| {
                (
                    class_id,
                    output[(4 + class_id) * OUTPUT_CANDIDATES + candidate],
                )
            })
            .max_by(|left, right| left.1.total_cmp(&right.1))
            .expect("NudeNet has at least one class");
        if score < CANDIDATE_SCORE_THRESHOLD {
            continue;
        }

        let center_x = output[candidate];
        let center_y = output[OUTPUT_CANDIDATES + candidate];
        let model_width = output[2 * OUTPUT_CANDIDATES + candidate];
        let model_height = output[3 * OUTPUT_CANDIDATES + candidate];
        let x1 = ((center_x - model_width / 2.0) * scale).clamp(0.0, original_size.0 as f32);
        let y1 = ((center_y - model_height / 2.0) * scale).clamp(0.0, original_size.1 as f32);
        let x2 = ((center_x + model_width / 2.0) * scale).clamp(0.0, original_size.0 as f32);
        let y2 = ((center_y + model_height / 2.0) * scale).clamp(0.0, original_size.1 as f32);
        candidates.push(Candidate {
            class_name: LABELS[class_id],
            score,
            bounding_box: [x1, y1, (x2 - x1).max(0.0), (y2 - y1).max(0.0)],
        });
    }

    Ok(candidates)
}

fn intersection_over_union(left: &[f32; 4], right: &[f32; 4]) -> f32 {
    let intersection_width = (left[0] + left[2]).min(right[0] + right[2]) - left[0].max(right[0]);
    let intersection_height = (left[1] + left[3]).min(right[1] + right[3]) - left[1].max(right[1]);
    if intersection_width <= 0.0 || intersection_height <= 0.0 {
        return 0.0;
    }

    let intersection = intersection_width * intersection_height;
    let union = left[2] * left[3] + right[2] * right[3] - intersection;
    if union > 0.0 {
        intersection / union
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, io::Cursor};

    use image::{DynamicImage, ImageFormat, Rgb, RgbImage, Rgba, RgbaImage};

    use super::{
        DEFAULT_MAX_ENCODED_BYTES, DEFAULT_MAX_PIXELS, Detector, ModelConfig, OUTPUT_CANDIDATES,
        OUTPUT_CHANNELS, PreprocessLimits, decode_candidates, decode_detections, preprocess,
    };

    fn rgb_png() -> Vec<u8> {
        let image = RgbImage::from_fn(2, 1, |x, _| match x {
            0 => Rgb([10, 20, 30]),
            1 => Rgb([40, 50, 60]),
            _ => unreachable!(),
        });
        let mut encoded = Cursor::new(Vec::new());
        DynamicImage::ImageRgb8(image)
            .write_to(&mut encoded, ImageFormat::Png)
            .unwrap();
        encoded.into_inner()
    }

    fn rgba_png() -> Vec<u8> {
        let image = RgbaImage::from_fn(2, 1, |x, _| match x {
            0 => Rgba([11, 22, 33, 0]),
            1 => Rgba([44, 55, 66, 255]),
            _ => unreachable!(),
        });
        let mut encoded = Cursor::new(Vec::new());
        DynamicImage::ImageRgba8(image)
            .write_to(&mut encoded, ImageFormat::Png)
            .unwrap();
        encoded.into_inner()
    }

    fn set_candidate(
        output: &mut [f32],
        candidate: usize,
        xywh: [f32; 4],
        class_id: usize,
        score: f32,
    ) {
        for (channel, value) in xywh.into_iter().enumerate() {
            output[channel * OUTPUT_CANDIDATES + candidate] = value;
        }
        output[(4 + class_id) * OUTPUT_CANDIDATES + candidate] = score;
    }

    // Production mutation caught: loading the runtime/session before authenticating model bytes
    // would accept an arbitrary ONNX path and later report the pinned model identity.
    #[test]
    fn rejects_model_content_that_does_not_match_pinned_sha256() {
        let temporary_directory = tempfile::tempdir().unwrap();
        let model_path = temporary_directory.path().join("different-model.onnx");
        fs::write(&model_path, b"not the pinned model").unwrap();

        let error = Detector::load(ModelConfig {
            model_path,
            runtime_path: temporary_directory
                .path()
                .join("runtime-must-not-be-loaded.so"),
            max_encoded_bytes: DEFAULT_MAX_ENCODED_BYTES,
            max_pixels: DEFAULT_MAX_PIXELS,
        })
        .err()
        .unwrap();

        assert_eq!(
            error.to_string(),
            "model SHA-256 c682bddd11bf3be78651695bb76250f83a99cc787c2fc8ab45219adc2dbbb549 does not match pinned c15d8273adad2d0a92f014cc69ab2d6c311a06777a55545f2c4eb46f51911f0f"
        );
    }

    // Production mutation caught: changing the tensor shape, channel order, anchor, padding,
    // normalization, or original-dimension metadata breaks the pinned preprocessing contract.
    #[test]
    fn preprocesses_rgb_png_into_top_left_anchored_bgr_nchw_tensor() {
        let encoded = rgb_png();
        let prepared = preprocess(&encoded, PreprocessLimits::default()).unwrap();

        assert_eq!(prepared.shape, [1, 3, 320, 320]);
        assert_eq!(prepared.original_size, (2, 1));
        assert_eq!(prepared.data.len(), 307_200);
        assert!((prepared.data[0] - 30.0 / 255.0).abs() < 1.0e-6);
        assert!((prepared.data[102_400] - 20.0 / 255.0).abs() < 1.0e-6);
        assert!((prepared.data[204_800] - 10.0 / 255.0).abs() < 1.0e-6);
        assert_eq!(prepared.data[319 * 320], 0.0);
        assert_eq!(prepared.data[102_400 + 319 * 320], 0.0);
        assert_eq!(prepared.data[204_800 + 319 * 320], 0.0);
        assert!(
            prepared
                .data
                .iter()
                .all(|value| (0.0..=1.0).contains(value))
        );
    }

    // Production mutation caught: treating RGBA alpha as premultiplication or failing to discard
    // it changes the model's three-channel BGR input.
    #[test]
    fn preprocesses_rgba_png_by_discarding_alpha_before_bgr_conversion() {
        let prepared = preprocess(&rgba_png(), PreprocessLimits::default()).unwrap();

        assert_eq!(prepared.shape, [1, 3, 320, 320]);
        assert_eq!(prepared.original_size, (2, 1));
        assert!((prepared.data[0] - 33.0 / 255.0).abs() < 1.0e-6);
        assert!((prepared.data[102_400] - 22.0 / 255.0).abs() < 1.0e-6);
        assert!((prepared.data[204_800] - 11.0 / 255.0).abs() < 1.0e-6);
        assert_eq!(prepared.data[319 * 320], 0.0);
    }

    // Production mutation caught: moving the encoded-size guard after decode permits an oversized
    // body to enter the image parser and defeats the fixed 16 MiB ingress bound.
    #[test]
    fn rejects_encoded_images_larger_than_sixteen_mebibytes() {
        let encoded = vec![0; 16 * 1024 * 1024 + 1];

        let error = preprocess(&encoded, PreprocessLimits::default()).unwrap_err();

        assert_eq!(
            error.to_string(),
            "encoded image is 16777217 bytes; limit is 16777216 bytes"
        );
    }

    // Production mutation caught: decoding before checking width times height can allocate a
    // decompression-bomb image larger than the fixed 40 megapixel bound.
    #[test]
    fn rejects_decoded_images_larger_than_forty_megapixels() {
        let png_for_8000_by_5001_rgb = [
            137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 31, 64, 0, 0, 19,
            137, 8, 2, 0, 0, 0, 120, 212, 31, 171, 0, 0, 0, 8, 73, 68, 65, 84, 120, 156, 3, 0, 0,
            0, 0, 1, 72, 6, 137, 210, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
        ];

        let error = preprocess(&png_for_8000_by_5001_rgb, PreprocessLimits::default()).unwrap_err();

        assert_eq!(
            error.to_string(),
            "decoded image is 40008000 pixels; limit is 40000000 pixels"
        );
    }

    // Production mutation caught: checking only source width times height lets a thin image under
    // 40 megapixels trigger an effectively unbounded square padding allocation.
    #[test]
    fn rejects_padding_that_would_exceed_the_pixel_bound() {
        let png_for_40000000_by_1_rgb = [
            137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 2, 98, 90, 0, 0, 0, 0, 1,
            8, 2, 0, 0, 0, 65, 145, 250, 156, 0, 0, 0, 8, 73, 68, 65, 84, 120, 156, 3, 0, 0, 0, 0,
            1, 72, 6, 137, 210, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
        ];

        let error =
            preprocess(&png_for_40000000_by_1_rgb, PreprocessLimits::default()).unwrap_err();

        assert_eq!(
            error.to_string(),
            "padded image is 1600000000000000 pixels; limit is 40000000 pixels"
        );
    }

    // Production mutation caught: changing the effective 0.25 final score gate,
    // box conversion/scaling/clipping, or the pinned first/last label mapping changes these
    // hand-derived detections.
    #[test]
    fn decodes_thresholded_scaled_clipped_boxes_with_pinned_labels() {
        let mut output = vec![0.0; OUTPUT_CHANNELS * OUTPUT_CANDIDATES];
        set_candidate(&mut output, 0, [10.0, 10.0, 30.0, 40.0], 0, 0.90);
        set_candidate(&mut output, 1, [300.0, 150.0, 80.0, 80.0], 17, 0.80);
        set_candidate(&mut output, 2, [150.0, 30.0, 10.0, 10.0], 1, 0.19);
        set_candidate(&mut output, 3, [170.0, 30.0, 10.0, 10.0], 1, 0.20);
        set_candidate(&mut output, 4, [190.0, 30.0, 10.0, 10.0], 1, 0.25);

        let detections = decode_detections(&output, (640, 320)).unwrap();

        assert_eq!(detections.len(), 2);
        assert_eq!(detections[0].class_name, "FEMALE_GENITALIA_COVERED");
        assert!((detections[0].score - 0.90).abs() < 1.0e-6);
        assert_eq!(detections[0].bounding_box, [0, 0, 50, 60]);
        assert_eq!(detections[1].class_name, "BUTTOCKS_COVERED");
        assert!((detections[1].score - 0.80).abs() < 1.0e-6);
        assert_eq!(detections[1].bounding_box, [520, 220, 120, 100]);
    }

    // Production mutation caught: lowering the preliminary 0.20 score gate admits the 0.19
    // candidate, while making the gate exclusive drops the candidate exactly at 0.20.
    #[test]
    fn candidate_score_filter_is_inclusive_at_point_two() {
        let mut output = vec![0.0; OUTPUT_CHANNELS * OUTPUT_CANDIDATES];
        set_candidate(&mut output, 0, [20.0, 20.0, 10.0, 10.0], 0, 0.19);
        set_candidate(&mut output, 1, [40.0, 20.0, 10.0, 10.0], 0, 0.20);
        set_candidate(&mut output, 2, [60.0, 20.0, 10.0, 10.0], 0, 0.25);

        let candidates = decode_candidates(&output, (320, 320)).unwrap();

        assert_eq!(candidates.len(), 2);
        assert!((candidates[0].score - 0.20).abs() < 1.0e-6);
        assert!((candidates[1].score - 0.25).abs() < 1.0e-6);
    }

    // Production mutation caught: class-aware suppression or moving the 0.45 IoU boundary keeps
    // the 0.4598-overlap candidate or removes the 0.4493-overlap candidate.
    #[test]
    fn applies_class_agnostic_nms_at_point_four_five_iou() {
        let mut output = vec![0.0; OUTPUT_CHANNELS * OUTPUT_CANDIDATES];
        set_candidate(&mut output, 0, [100.0, 100.0, 100.0, 100.0], 0, 0.90);
        set_candidate(&mut output, 1, [137.0, 100.0, 100.0, 100.0], 17, 0.80);

        let detections = decode_detections(&output, (320, 320)).unwrap();

        assert_eq!(detections.len(), 1);
        assert_eq!(detections[0].class_name, "FEMALE_GENITALIA_COVERED");
        assert!((detections[0].score - 0.90).abs() < 1.0e-6);
        assert_eq!(detections[0].bounding_box, [50, 50, 100, 100]);

        let mut below_threshold = vec![0.0; OUTPUT_CHANNELS * OUTPUT_CANDIDATES];
        set_candidate(
            &mut below_threshold,
            0,
            [100.0, 100.0, 100.0, 100.0],
            0,
            0.90,
        );
        set_candidate(
            &mut below_threshold,
            1,
            [138.0, 100.0, 100.0, 100.0],
            17,
            0.80,
        );

        let detections = decode_detections(&below_threshold, (320, 320)).unwrap();

        assert_eq!(detections.len(), 2);
        assert_eq!(detections[1].bounding_box, [88, 50, 100, 100]);
    }
}
