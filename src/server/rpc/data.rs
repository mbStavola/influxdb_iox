//! This module contains code to translate from InfluxDB IOx data
//! formats into the formats needed by gRPC

use std::{collections::BTreeSet, sync::Arc};

use arrow_deps::arrow::{
    array::{ArrayRef, BooleanArray, Float64Array, Int64Array, StringArray},
    datatypes::DataType as ArrowDataType,
};

use query::exec::{
    fieldlist::FieldList,
    seriesset::{GroupDescription, GroupedSeriesSetItem, SeriesSet},
};

use generated_types::{
    measurement_fields_response::{FieldType, MessageField},
    read_response::{
        frame::Data, BooleanPointsFrame, DataType, FloatPointsFrame, Frame, GroupFrame,
        IntegerPointsFrame, SeriesFrame, StringPointsFrame,
    },
    MeasurementFieldsResponse, ReadResponse, Tag,
};

use snafu::Snafu;

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("Unsupported data type in gRPC data translation: {}", type_name))]
    UnsupportedDataType { type_name: String },

    #[snafu(display("Unsupported field data type in gRPC data translation: {}", type_name))]
    UnsupportedFieldType { type_name: String },
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Convert a set of tag_keys into a form suitable for gRPC transport
///
/// Namely, a Vec<Vec<u8>>, including the measurement and field names
pub fn tag_keys_to_byte_vecs(tag_keys: Arc<BTreeSet<String>>) -> Vec<Vec<u8>> {
    // special case measurement and field
    let specials_iter = vec![b"_field".to_vec(), b"_measurement".to_vec()].into_iter();

    let tag_keys_iter = tag_keys.iter().map(|name| name.bytes().collect());

    specials_iter.chain(tag_keys_iter).collect()
}

/// Convert `SeriesSet` into a form suitable for gRPC transport
///
/// Each `SeriesSet` gets converted into this pattern:
///
/// ```
/// (SeriesFrame for field1)
/// (*Points for field1)
/// (SeriesFrame for field12)
/// (*Points for field1)
/// (....)
/// ```
///
/// The specific type of (*Points) depends on the type of field column.
pub fn series_set_to_read_response(series_set: SeriesSet) -> Result<ReadResponse> {
    let frames = series_set_to_frames(series_set)?;
    Ok(ReadResponse { frames })
}

fn series_set_to_frames(series_set: SeriesSet) -> Result<Vec<Frame>> {
    let mut data_records = Vec::new();
    for field_index in series_set.field_indices.iter() {
        field_to_data(&mut data_records, &series_set, *field_index)?
    }

    let frames = data_records
        .into_iter()
        .map(|data| {
            let data = Some(data);

            Frame { data }
        })
        .collect();

    Ok(frames)
}

/// Convert `GroupedSeriesSetIem` into a form suitable for gRPC transport
///
/// Each `GroupedSeriesSetItem` gets converted into this pattern:
///
/// ```
/// (GroupFrame)
///
/// (SeriesFrame for field1)
/// (*Points for field1)
/// (SeriesFrame for field12)
/// (*Points for field1)
/// (....)
/// (SeriesFrame for field1)
/// (*Points for field1)
/// (SeriesFrame for field12)
/// (*Points for field1)
/// (....)
/// ```
///
/// The specific type of (*Points) depends on the type of field column.
pub fn grouped_series_set_item_to_read_response(
    grouped_series_set_item: GroupedSeriesSetItem,
) -> Result<ReadResponse> {
    let frames = match grouped_series_set_item {
        GroupedSeriesSetItem::GroupStart(group_description) => {
            group_description_to_frames(group_description)?
        }
        GroupedSeriesSetItem::GroupData(series_set) => series_set_to_frames(series_set)?,
    };
    Ok(ReadResponse { frames })
}

fn group_description_to_frames(group_description: GroupDescription) -> Result<Vec<Frame>> {
    // split key=value pairs into two separate vectors
    let (tag_keys, partition_key_vals): (Vec<Vec<u8>>, Vec<Vec<u8>>) = group_description
        .tags
        .into_iter()
        .map(|(k, v)| (k.bytes().collect(), v.bytes().collect()))
        .unzip();

    let group_frame = GroupFrame {
        tag_keys,
        partition_key_vals,
    };

    let data = Some(Data::Group(group_frame));

    Ok(vec![Frame { data }])
}

fn data_type(array: &ArrayRef) -> Result<DataType> {
    match array.data_type() {
        ArrowDataType::Utf8 => Ok(DataType::String),
        ArrowDataType::Float64 => Ok(DataType::Float),
        ArrowDataType::Int64 => Ok(DataType::Integer),
        ArrowDataType::Boolean => Ok(DataType::Boolean),
        _ => UnsupportedDataType {
            type_name: format!("{:?}", array.data_type()),
        }
        .fail(),
    }
}

// Convert and append a single field to a sequence of frames
fn field_to_data(frames: &mut Vec<Data>, series_set: &SeriesSet, field_index: usize) -> Result<()> {
    let batch = &series_set.batch;
    let schema = batch.schema();

    let array = batch.column(field_index);

    let start_row = series_set.start_row;
    let num_rows = series_set.num_rows;

    let series_frame = SeriesFrame {
        tags: convert_tags(
            series_set.table_name.as_ref(),
            schema.field(field_index).name(),
            &series_set.tags,
        ),
        data_type: data_type(array)? as i32,
    };
    frames.push(Data::Series(series_frame));

    let timestamps = batch
        .column(series_set.timestamp_index)
        .as_any()
        .downcast_ref::<Int64Array>()
        .unwrap()
        .extract_values(start_row, num_rows);

    frames.push(match array.data_type() {
        ArrowDataType::Utf8 => {
            let values = array
                .as_any()
                .downcast_ref::<StringArray>()
                .unwrap()
                .extract_values(start_row, num_rows);
            Data::StringPoints(StringPointsFrame { timestamps, values })
        }
        ArrowDataType::Float64 => {
            let values = array
                .as_any()
                .downcast_ref::<Float64Array>()
                .unwrap()
                .extract_values(start_row, num_rows);
            Data::FloatPoints(FloatPointsFrame { timestamps, values })
        }
        ArrowDataType::Int64 => {
            let values = array
                .as_any()
                .downcast_ref::<Int64Array>()
                .unwrap()
                .extract_values(start_row, num_rows);
            Data::IntegerPoints(IntegerPointsFrame { timestamps, values })
        }
        ArrowDataType::Boolean => {
            let values = array
                .as_any()
                .downcast_ref::<BooleanArray>()
                .unwrap()
                .extract_values(start_row, num_rows);
            Data::BooleanPoints(BooleanPointsFrame { timestamps, values })
        }
        _ => {
            return UnsupportedDataType {
                type_name: format!("{:?}", array.data_type()),
            }
            .fail();
        }
    });
    Ok(())
}

// Convert the tag=value pairs from the series set to the correct gRPC
// format, and add the _f and _m tags for the field name and measurement
fn convert_tags(
    table_name: &str,
    field_name: &str,
    tags: &[(Arc<String>, Arc<String>)],
) -> Vec<Tag> {
    let mut converted_tags = Vec::new();

    // Special case "measurement" name which is modeled as a tag of
    // "_measurement" and "field" which is modeled as a tag of "_field"
    converted_tags.push(Tag {
        key: b"_field".to_vec(),
        value: field_name.bytes().collect(),
    });
    converted_tags.push(Tag {
        key: b"_measurement".to_vec(),
        value: table_name.bytes().collect(),
    });

    // convert the rest of the tags
    converted_tags.extend(tags.iter().map(|(k, v)| {
        let key = k.bytes().collect();
        let value = v.bytes().collect();

        Tag { key, value }
    }));

    converted_tags
}

trait ExtractValues<T> {
    /// Extracts num_rows of data starting from start_row as a vector
    fn extract_values(&self, start_row: usize, num_rows: usize) -> Vec<T>;
}

impl ExtractValues<String> for StringArray {
    fn extract_values(&self, start_row: usize, num_rows: usize) -> Vec<String> {
        let end_row = start_row + num_rows;
        (start_row..end_row)
            .map(|row| self.value(row).to_string())
            .collect()
    }
}

impl ExtractValues<i64> for Int64Array {
    fn extract_values(&self, start_row: usize, num_rows: usize) -> Vec<i64> {
        let end_row = start_row + num_rows;
        (start_row..end_row).map(|row| self.value(row)).collect()
    }
}

impl ExtractValues<f64> for Float64Array {
    fn extract_values(&self, start_row: usize, num_rows: usize) -> Vec<f64> {
        let end_row = start_row + num_rows;
        (start_row..end_row).map(|row| self.value(row)).collect()
    }
}

impl ExtractValues<bool> for BooleanArray {
    fn extract_values(&self, start_row: usize, num_rows: usize) -> Vec<bool> {
        let end_row = start_row + num_rows;
        (start_row..end_row).map(|row| self.value(row)).collect()
    }
}

/// Translates FieldList into the gRPC format
pub fn fieldlist_to_measurement_fields_response(
    fieldlist: FieldList,
) -> Result<MeasurementFieldsResponse> {
    let fields = fieldlist
        .fields
        .into_iter()
        .map(|f| {
            Ok(MessageField {
                key: f.name,
                r#type: datatype_to_measurement_field_enum(&f.data_type)? as i32,
                timestamp: f.last_timestamp,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(MeasurementFieldsResponse { fields })
}

fn datatype_to_measurement_field_enum(data_type: &ArrowDataType) -> Result<FieldType> {
    match data_type {
        ArrowDataType::Float64 => Ok(FieldType::Float),
        ArrowDataType::Int64 => Ok(FieldType::Integer),
        ArrowDataType::UInt64 => Ok(FieldType::Unsigned),
        ArrowDataType::Utf8 => Ok(FieldType::String),
        ArrowDataType::Boolean => Ok(FieldType::Boolean),
        _ => UnsupportedFieldType {
            type_name: format!("{:?}", data_type),
        }
        .fail(),
    }
}

#[cfg(test)]
mod tests {
    use arrow_deps::arrow::{
        datatypes::{DataType as ArrowDataType, Field as ArrowField, Schema},
        record_batch::RecordBatch,
    };
    use query::exec::fieldlist::Field;

    use super::*;

    #[test]
    fn test_series_set_conversion() {
        let series_set = SeriesSet {
            table_name: Arc::new("the_table".into()),
            tags: vec![(Arc::new("tag1".into()), Arc::new("val1".into()))],
            timestamp_index: 4,
            field_indices: Arc::new(vec![0, 1, 2, 3]),
            start_row: 1,
            num_rows: 2,
            batch: make_record_batch(),
        };

        let response =
            series_set_to_read_response(series_set).expect("Correctly converted series set");
        println!("Response is: {:#?}", response);

        assert_eq!(response.frames.len(), 8); // 2 per field x 4 fields = 8

        let dumped_frames = response
            .frames
            .iter()
            .map(|f| dump_frame(f))
            .collect::<Vec<_>>();

        let expected_frames = vec![
            "SeriesFrame, tags: _field=string_field,_measurement=the_table,tag1=val1, type: 4",
            "StringPointsFrame, timestamps: [2000, 3000], values: bar,baz",
            "SeriesFrame, tags: _field=int_field,_measurement=the_table,tag1=val1, type: 1",
            "IntegerPointsFrame, timestamps: [2000, 3000], values: \"2,3\"",
            "SeriesFrame, tags: _field=float_field,_measurement=the_table,tag1=val1, type: 0",
            "FloatPointsFrame, timestamps: [2000, 3000], values: \"20.1,30.1\"",
            "SeriesFrame, tags: _field=boolean_field,_measurement=the_table,tag1=val1, type: 3",
            "BooleanPointsFrame, timestamps: [2000, 3000], values: false,true",
        ];

        assert_eq!(
            dumped_frames, expected_frames,
            "Expected:\n{:#?}\nActual:\n{:#?}",
            expected_frames, dumped_frames
        );
    }

    #[test]
    fn test_group_group_conversion() {
        let group_description = GroupDescription {
            tags: vec![
                (Arc::new("tag1".into()), Arc::new("val1".into())),
                (Arc::new("tag2".into()), Arc::new("val2".into())),
            ],
        };

        let grouped_series_set_item = GroupedSeriesSetItem::GroupStart(group_description);

        let response = grouped_series_set_item_to_read_response(grouped_series_set_item)
            .expect("Correctly converted grouped_series_set_item");
        println!("Response is: {:#?}", response);

        let dumped_frames = response
            .frames
            .iter()
            .map(|f| dump_frame(f))
            .collect::<Vec<_>>();

        let expected_frames =
            vec!["GroupFrame, tag_keys: tag1,tag2, partition_key_vals: val1,val2"];

        assert_eq!(
            dumped_frames, expected_frames,
            "Expected:\n{:#?}\nActual:\n{:#?}",
            expected_frames, dumped_frames
        );
    }

    #[test]
    fn test_group_series_conversion() {
        let schema = Arc::new(Schema::new(vec![
            ArrowField::new("float_field", ArrowDataType::Float64, true),
            ArrowField::new("time", ArrowDataType::Int64, true),
        ]));

        let float_array: ArrayRef = Arc::new(Float64Array::from(vec![10.1, 20.1, 30.1, 40.1]));
        let timestamp_array: ArrayRef = Arc::new(Int64Array::from(vec![1000, 2000, 3000, 4000]));

        let batch = RecordBatch::try_new(schema, vec![float_array, timestamp_array])
            .expect("created new record batch");

        let series_set = SeriesSet {
            table_name: Arc::new("the_table".into()),
            tags: vec![(Arc::new("tag1".into()), Arc::new("val1".into()))],
            timestamp_index: 1,
            field_indices: Arc::new(vec![0]),
            start_row: 1,
            num_rows: 2,
            batch,
        };

        let grouped_series_set_item = GroupedSeriesSetItem::GroupData(series_set);

        let response = grouped_series_set_item_to_read_response(grouped_series_set_item)
            .expect("Correctly converted grouped_series_set_item");

        println!("Response is: {:#?}", response);

        let dumped_frames = response
            .frames
            .iter()
            .map(|f| dump_frame(f))
            .collect::<Vec<_>>();

        let expected_frames = vec![
            "SeriesFrame, tags: _field=float_field,_measurement=the_table,tag1=val1, type: 0",
            "FloatPointsFrame, timestamps: [2000, 3000], values: \"20.1,30.1\"",
        ];

        assert_eq!(
            dumped_frames, expected_frames,
            "Expected:\n{:#?}\nActual:\n{:#?}",
            expected_frames, dumped_frames
        );
    }

    #[test]
    fn test_field_list_conversion() {
        let input = FieldList {
            fields: vec![
                Field {
                    name: "float".into(),
                    data_type: ArrowDataType::Float64,
                    last_timestamp: 1000,
                },
                Field {
                    name: "int".into(),
                    data_type: ArrowDataType::Int64,
                    last_timestamp: 2000,
                },
                Field {
                    name: "uint".into(),
                    data_type: ArrowDataType::UInt64,
                    last_timestamp: 3000,
                },
                Field {
                    name: "string".into(),
                    data_type: ArrowDataType::Utf8,
                    last_timestamp: 4000,
                },
                Field {
                    name: "bool".into(),
                    data_type: ArrowDataType::Boolean,
                    last_timestamp: 5000,
                },
            ],
        };

        let expected = MeasurementFieldsResponse {
            fields: vec![
                MessageField {
                    key: "float".into(),
                    r#type: FieldType::Float as i32,
                    timestamp: 1000,
                },
                MessageField {
                    key: "int".into(),
                    r#type: FieldType::Integer as i32,
                    timestamp: 2000,
                },
                MessageField {
                    key: "uint".into(),
                    r#type: FieldType::Unsigned as i32,
                    timestamp: 3000,
                },
                MessageField {
                    key: "string".into(),
                    r#type: FieldType::String as i32,
                    timestamp: 4000,
                },
                MessageField {
                    key: "bool".into(),
                    r#type: FieldType::Boolean as i32,
                    timestamp: 5000,
                },
            ],
        };

        let actual = fieldlist_to_measurement_fields_response(input).unwrap();
        assert_eq!(
            actual, expected,
            "Expected:\n{:#?}\nActual:\n{:#?}",
            expected, actual
        );
    }

    #[test]
    fn test_field_list_conversion_error() {
        let input = FieldList {
            fields: vec![Field {
                name: "unsupported".into(),
                data_type: ArrowDataType::Int8,
                last_timestamp: 1000,
            }],
        };
        let result = fieldlist_to_measurement_fields_response(input);
        match result {
            Ok(r) => panic!("Unexpected success: {:?}", r),
            Err(e) => {
                let expected = "Unsupported field data type in gRPC data translation: Int8";
                let actual = format!("{}", e);
                assert!(
                    actual.contains(expected),
                    "Could not find expected '{}' in actual '{}'",
                    expected,
                    actual
                );
            }
        }
    }

    fn dump_frame(frame: &Frame) -> String {
        let data = &frame.data;
        match data {
            Some(Data::Series(SeriesFrame { tags, data_type })) => format!(
                "SeriesFrame, tags: {}, type: {:?}",
                dump_tags(tags),
                data_type
            ),
            Some(Data::FloatPoints(FloatPointsFrame { timestamps, values })) => format!(
                "FloatPointsFrame, timestamps: {:?}, values: {:?}",
                timestamps,
                dump_values(values)
            ),
            Some(Data::IntegerPoints(IntegerPointsFrame { timestamps, values })) => format!(
                "IntegerPointsFrame, timestamps: {:?}, values: {:?}",
                timestamps,
                dump_values(values)
            ),
            Some(Data::BooleanPoints(BooleanPointsFrame { timestamps, values })) => format!(
                "BooleanPointsFrame, timestamps: {:?}, values: {}",
                timestamps,
                dump_values(values)
            ),
            Some(Data::StringPoints(StringPointsFrame { timestamps, values })) => format!(
                "StringPointsFrame, timestamps: {:?}, values: {}",
                timestamps,
                dump_values(values)
            ),
            Some(Data::Group(GroupFrame {
                tag_keys,
                partition_key_vals,
            })) => format!(
                "GroupFrame, tag_keys: {}, partition_key_vals: {}",
                dump_u8_vec(tag_keys),
                dump_u8_vec(partition_key_vals),
            ),
            None => "<NO data field>".into(),
            _ => ":thinking_face: unknown frame type".into(),
        }
    }

    fn dump_values<T>(v: &[T]) -> String
    where
        T: std::fmt::Display,
    {
        v.iter()
            .map(|item| format!("{}", item))
            .collect::<Vec<_>>()
            .join(",")
    }

    fn dump_u8_vec(encoded_strings: &[Vec<u8>]) -> String {
        encoded_strings
            .iter()
            .map(|b| String::from_utf8_lossy(b))
            .collect::<Vec<_>>()
            .join(",")
    }

    fn dump_tags(tags: &[Tag]) -> String {
        tags.iter()
            .map(|tag| {
                format!(
                    "{}={}",
                    String::from_utf8_lossy(&tag.key),
                    String::from_utf8_lossy(&tag.value),
                )
            })
            .collect::<Vec<_>>()
            .join(",")
    }

    fn make_record_batch() -> RecordBatch {
        let schema = Arc::new(Schema::new(vec![
            ArrowField::new("string_field", ArrowDataType::Utf8, true),
            ArrowField::new("int_field", ArrowDataType::Int64, true),
            ArrowField::new("float_field", ArrowDataType::Float64, true),
            ArrowField::new("boolean_field", ArrowDataType::Boolean, true),
            ArrowField::new("time", ArrowDataType::Int64, true),
        ]));

        let string_array: ArrayRef = Arc::new(StringArray::from(vec!["foo", "bar", "baz", "foo"]));
        let int_array: ArrayRef = Arc::new(Int64Array::from(vec![1, 2, 3, 4]));
        let float_array: ArrayRef = Arc::new(Float64Array::from(vec![10.1, 20.1, 30.1, 40.1]));
        let bool_array: ArrayRef = Arc::new(BooleanArray::from(vec![true, false, true, false]));

        let timestamp_array: ArrayRef = Arc::new(Int64Array::from(vec![1000, 2000, 3000, 4000]));

        RecordBatch::try_new(
            schema,
            vec![
                string_array,
                int_array,
                float_array,
                bool_array,
                timestamp_array,
            ],
        )
        .expect("created new record batch")
    }
}
