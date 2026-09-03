use std::io::Write;

use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MetricStage {
    Inference,
    Policy,
}

#[derive(Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MetricVerdict {
    Allow,
    Replace,
}

#[derive(Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct MetricRecord {
    pub stage: MetricStage,
    pub verdict: MetricVerdict,
    pub fixture_index: usize,
    pub elapsed_micros: u64,
}

pub struct MetricSink<W> {
    writer: W,
}

impl<W: Write> MetricSink<W> {
    pub fn new(writer: W) -> Self {
        Self { writer }
    }

    pub fn write(&mut self, record: &MetricRecord) -> serde_json::Result<()> {
        serde_json::to_writer(&mut self.writer, record)?;
        self.writer.write_all(b"\n").map_err(serde_json::Error::io)
    }

    pub fn into_inner(self) -> W {
        self.writer
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use serde_json::Value;

    use super::{MetricRecord, MetricSink, MetricStage, MetricVerdict};

    // Production mutation caught: adding any URL, filesystem path, response body, tensor, or
    // another unapproved field would expand the persisted privacy surface beyond fixture timings.
    #[test]
    fn writes_one_privacy_safe_json_record_per_line() {
        let record = MetricRecord {
            stage: MetricStage::Inference,
            verdict: MetricVerdict::Replace,
            fixture_index: 1,
            elapsed_micros: 42,
        };
        let mut sink = MetricSink::new(Vec::new());

        sink.write(&record).unwrap();

        let bytes = sink.into_inner();
        assert_eq!(bytes.last(), Some(&b'\n'));
        let line = std::str::from_utf8(&bytes[..bytes.len() - 1]).unwrap();
        let decoded: MetricRecord = serde_json::from_str(line).unwrap();
        assert_eq!(decoded, record);
        let value: Value = serde_json::from_str(line).unwrap();
        assert_eq!(value["stage"], "inference");
        assert_eq!(value["verdict"], "replace");
        assert_eq!(value["fixture_index"], 1);
        assert_eq!(value["elapsed_micros"], 42);
        let keys = value
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>();
        assert_eq!(
            keys,
            BTreeSet::from([
                "elapsed_micros".to_owned(),
                "fixture_index".to_owned(),
                "stage".to_owned(),
                "verdict".to_owned(),
            ])
        );
    }
}
