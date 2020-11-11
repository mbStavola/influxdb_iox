pub mod rle;

use std::collections::{BTreeMap, BTreeSet};

use croaring::Bitmap;

// This makes the RLE type available under the dictionary module.
pub use self::rle::RLE;

use crate::column::{cmp, RowIDs};

/// The encoded id for a NULL value.
pub const NULL_ID: u32 = 0;

enum Encoding {
    RLE(RLE),
}

impl Encoding {
    fn size(&self) -> u64 {
        match &self {
            Encoding::RLE(enc) => enc.size(),
        }
    }

    fn push(&mut self, v: String) {
        match self {
            Encoding::RLE(ref mut enc) => enc.push(v),
        }
    }

    fn push_none(&mut self) {
        match self {
            Encoding::RLE(ref mut enc) => enc.push_none(),
        }
    }

    /// Adds additional repetitions of the provided value to the encoded data.
    /// It is the caller's responsibility to ensure that the dictionary encoded
    /// remains sorted.
    fn push_additional(&mut self, v: Option<String>, additional: u32) {
        match self {
            Encoding::RLE(ref mut env) => env.push_additional(v, additional),
        }
    }

    /// Determine if NULL is encoded in the column.
    fn contains_null(&self) -> bool {
        match self {
            Encoding::RLE(enc) => enc.contains_null(),
        }
    }

    //
    //
    // ---- Methods for getting row ids from values.
    //
    //

    /// Populates the provided destination container with the row ids satisfying
    /// the provided predicate.
    fn row_ids_filter(&self, value: &str, op: &cmp::Operator, dst: RowIDs) -> RowIDs {
        match self {
            Encoding::RLE(enc) => enc.row_ids_filter(value, op, dst),
        }
    }

    /// Populates the provided destination container with the row ids for rows
    /// that null.
    fn row_ids_null(&self, dst: RowIDs) -> RowIDs {
        match self {
            Encoding::RLE(enc) => enc.row_ids_null(dst),
        }
    }

    /// Populates the provided destination container with the row ids for rows
    /// that are not null.
    fn row_ids_not_null(&self, dst: RowIDs) -> RowIDs {
        match self {
            Encoding::RLE(enc) => enc.row_ids_not_null(dst),
        }
    }

    // All row ids that have either NULL or not NULL values.
    fn row_ids_is_null(&self, is_null: bool, dst: RowIDs) -> RowIDs {
        match self {
            Encoding::RLE(enc) => enc.row_ids_null(dst),
        }
    }

    // The set of row ids for each distinct value in the column.
    fn group_row_ids(&self) -> &BTreeMap<u32, Bitmap> {
        match self {
            Encoding::RLE(enc) => enc.group_row_ids(),
        }
    }

    //
    //
    // ---- Methods for getting materialised values.
    //
    //

    fn dictionary(&self) -> &[String] {
        match self {
            Encoding::RLE(enc) => enc.dictionary(),
        }
    }

    /// Returns the logical value present at the provided row id.
    ///
    /// N.B right now this doesn't discern between an invalid row id and a NULL
    /// value at a valid location.
    fn value(&self, row_id: u32) -> Option<&String> {
        match self {
            Encoding::RLE(enc) => enc.value(row_id),
        }
    }

    /// Materialises the decoded value belonging to the provided encoded id.
    ///
    /// Panics if there is no decoded value for the provided id
    fn decode_id(&self, encoded_id: u32) -> Option<String> {
        match self {
            Encoding::RLE(enc) => enc.decode_id(encoded_id),
        }
    }

    /// Materialises a vector of references to the decoded values in the
    /// provided row ids.
    ///
    /// NULL values are represented by None. It is the caller's responsibility
    /// to ensure row ids are a monotonically increasing set.
    fn values<'a>(&'a self, row_ids: &[u32], dst: Vec<Option<&'a str>>) -> Vec<Option<&'a str>> {
        match self {
            Encoding::RLE(enc) => enc.values(row_ids, dst),
        }
    }

    /// Returns the lexicographical minimum value for the provided set of row
    /// ids. NULL values are not considered the minimum value if any non-null
    /// value exists at any of the provided row ids.
    fn min<'a>(&'a self, row_ids: &[u32]) -> Option<&'a String> {
        match self {
            Encoding::RLE(enc) => enc.min(row_ids),
        }
    }

    /// Returns the lexicographical maximum value for the provided set of row
    /// ids. NULL values are not considered the maximum value if any non-null
    /// value exists at any of the provided row ids.
    fn max<'a>(&'a self, row_ids: &[u32]) -> Option<&'a String> {
        match self {
            Encoding::RLE(enc) => enc.max(row_ids),
        }
    }

    /// Returns the total number of non-null values found at the provided set of
    /// row ids.
    fn count(&self, row_ids: &[u32]) -> u32 {
        match self {
            Encoding::RLE(enc) => enc.count(row_ids),
        }
    }

    /// Returns references to the logical (decoded) values for all the rows in
    /// the column.
    ///
    /// NULL values are represented by None.
    ///
    fn all_values<'a>(&'a mut self, dst: Vec<Option<&'a String>>) -> Vec<Option<&'a String>> {
        match self {
            Encoding::RLE(enc) => enc.all_values(dst),
        }
    }

    /// Returns references to the unique set of values encoded at each of the
    /// provided ids.
    ///
    /// It is the caller's responsibility to ensure row ids are a monotonically
    /// increasing set.
    fn distinct_values<'a>(
        &'a self,
        row_ids: &[u32],
        dst: BTreeSet<Option<&'a String>>,
    ) -> BTreeSet<Option<&'a String>> {
        match self {
            Encoding::RLE(enc) => enc.distinct_values(row_ids, dst),
        }
    }

    //
    //
    // ---- Methods for getting encoded values directly, typically to be used
    //      as part of group keys.
    //
    //

    /// Return the raw encoded values for the provided logical row ids.
    /// Encoded values for NULL values are included.
    ///
    fn encoded_values(&self, row_ids: &[u32], dst: Vec<u32>) -> Vec<u32> {
        match self {
            Encoding::RLE(enc) => enc.encoded_values(row_ids, dst),
        }
    }

    /// Returns all encoded values for the column including the encoded value
    /// for any NULL values.
    fn all_encoded_values(&self, dst: Vec<u32>) -> Vec<u32> {
        match self {
            Encoding::RLE(enc) => enc.all_encoded_values(dst),
        }
    }

    //
    //
    // ---- Methods for optimising schema exploration.
    //
    //

    /// Efficiently determines if this column contains non-null values that
    /// differ from the provided set of values.
    ///
    /// Informally, this method provides an efficient way of answering "is it
    /// worth spending time reading this column for values or do I already have
    /// all the values in a set".
    ///
    /// More formally, this method returns the relative complement of this
    /// column's values in the provided set of values.
    ///
    /// This method would be useful when the same column is being read across
    /// many segments, and one wants to determine to the total distinct set of
    /// values. By exposing the current result set to each column (as an
    /// argument to `contains_other_values`) columns can be short-circuited when
    /// they only contain values that have already been discovered.
    ///
    fn contains_other_values(&self, values: &BTreeSet<Option<&String>>) -> bool {
        match self {
            Encoding::RLE(enc) => enc.contains_other_values(values),
        }
    }

    /// Determines if the column contains at least one non-null value at
    /// any of the provided row ids.
    ///
    /// It is the caller's responsibility to ensure row ids are a monotonically
    /// increasing set.
    fn has_non_null_value(&self, row_ids: &[u32]) -> bool {
        match self {
            Encoding::RLE(enc) => enc.has_non_null_value(row_ids),
        }
    }
}

#[cfg(test)]
mod test {
    use std::collections::BTreeSet;

    use crate::column::{cmp, RowIDs};

    use super::*;

    #[test]
    fn size() {
        let mut enc = Encoding::RLE(RLE::default());
        enc.push_additional(Some("east".to_string()), 3);
        enc.push_additional(Some("north".to_string()), 1);
        enc.push_additional(Some("east".to_string()), 5);
        enc.push_additional(Some("south".to_string()), 2);
        enc.push_none();
        enc.push_none();
        enc.push_none();
        enc.push_none();

        // keys - 18 bytes.
        // entry_index is 24 + ((24+4) * 3) + 14 == 122
        // index_entry is 24 + (24*4) + 14 == 134
        // index_row_ids is 24 + (4 + 0?? * 4) == 40 ??????
        // run lengths is 24 + (8*5) == 64
        // 360

        // TODO(edd): there some mystery bytes in the bitmap implementation.
        // need to figure out how to measure these
        assert_eq!(enc.size(), 397);
    }

    #[test]
    fn rle_push() {
        let mut enc = Encoding::RLE(RLE::from(vec!["hello", "hello", "hello", "hello"]));
        enc.push_additional(Some("hello".to_string()), 1);
        enc.push_additional(None, 3);
        enc.push("world".to_string());

        assert_eq!(
            enc.all_values(vec![]),
            [
                Some(&"hello".to_string()),
                Some(&"hello".to_string()),
                Some(&"hello".to_string()),
                Some(&"hello".to_string()),
                Some(&"hello".to_string()),
                None,
                None,
                None,
                Some(&"world".to_string()),
            ]
        );

        enc.push_additional(Some("zoo".to_string()), 3);
        enc.push_none();
        assert_eq!(
            enc.all_values(vec![]),
            [
                Some(&"hello".to_string()),
                Some(&"hello".to_string()),
                Some(&"hello".to_string()),
                Some(&"hello".to_string()),
                Some(&"hello".to_string()),
                None,
                None,
                None,
                Some(&"world".to_string()),
                Some(&"zoo".to_string()),
                Some(&"zoo".to_string()),
                Some(&"zoo".to_string()),
                None,
            ]
        );
    }

    // tests a defect I discovered.
    #[test]
    fn push_additional_first_run_length() {
        let arr = vec!["world".to_string(), "hello".to_string()];

        let mut enc = Encoding::RLE(RLE::with_dictionary(
            arr.into_iter().collect::<BTreeSet<String>>(),
        ));
        enc.push_additional(Some("world".to_string()), 1);
        enc.push_additional(Some("hello".to_string()), 1);

        assert_eq!(
            enc.all_values(vec![]),
            vec![Some(&"world".to_string()), Some(&"hello".to_string())]
        );
        assert_eq!(enc.all_encoded_values(vec![]), vec![2, 1]);

        enc = Encoding::RLE(RLE::default());
        enc.push_additional(Some("hello".to_string()), 1);
        enc.push_additional(Some("world".to_string()), 1);

        assert_eq!(
            enc.all_values(vec![]),
            vec![Some(&"hello".to_string()), Some(&"world".to_string())]
        );
        assert_eq!(enc.all_encoded_values(vec![]), vec![1, 2]);
    }

    #[test]
    #[should_panic]
    fn rle_push_wrong_order() {
        let mut enc = Encoding::RLE(RLE::default());
        enc.push("b".to_string());
        enc.push("a".to_string());
    }

    #[test]
    fn row_ids_filter_equal() {
        let mut enc = Encoding::RLE(RLE::default());
        enc.push_additional(Some("east".to_string()), 3); // 0, 1, 2
        enc.push_additional(Some("north".to_string()), 1); // 3
        enc.push_additional(Some("east".to_string()), 5); // 4, 5, 6, 7, 8
        enc.push_none(); // 9
        enc.push_additional(Some("south".to_string()), 2); // 10, 11

        let ids = enc.row_ids_filter(&"east", &cmp::Operator::Equal, RowIDs::Vector(vec![]));
        assert_eq!(ids, RowIDs::Vector(vec![0, 1, 2, 4, 5, 6, 7, 8]));

        let ids = enc.row_ids_filter(&"south", &cmp::Operator::Equal, RowIDs::Vector(vec![]));
        assert_eq!(ids, RowIDs::Vector(vec![10, 11]));

        let ids = enc.row_ids_filter(&"foo", &cmp::Operator::Equal, RowIDs::Vector(vec![]));
        assert!(ids.is_empty());

        // != some value not in the column should exclude the NULL value.
        let ids = enc.row_ids_filter(&"foo", &cmp::Operator::NotEqual, RowIDs::Vector(vec![]));
        assert_eq!(ids, RowIDs::Vector(vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 10, 11]));

        let ids = enc.row_ids_filter(&"east", &cmp::Operator::NotEqual, RowIDs::Vector(vec![]));
        assert_eq!(ids, RowIDs::Vector(vec![3, 10, 11]));
    }

    #[test]
    fn row_ids_filter_equal_no_null() {
        let mut enc = Encoding::RLE(RLE::default());
        enc.push_additional(Some("east".to_string()), 2);
        enc.push_additional(Some("west".to_string()), 1);

        let ids = enc.row_ids_filter(&"abba", &cmp::Operator::NotEqual, RowIDs::Vector(vec![]));
        assert_eq!(ids, RowIDs::Vector(vec![0, 1, 2]));
    }

    #[test]
    fn row_ids_filter_cmp() {
        let mut enc = Encoding::RLE(RLE::default());
        enc.push_additional(Some("east".to_string()), 3); // 0, 1, 2
        enc.push_additional(Some("north".to_string()), 1); // 3
        enc.push_additional(Some("east".to_string()), 5); // 4, 5, 6, 7, 8
        enc.push_additional(Some("south".to_string()), 2); // 9, 10
        enc.push_additional(Some("west".to_string()), 1); // 11
        enc.push_additional(Some("north".to_string()), 1); // 12
        enc.push_none(); // 13
        enc.push_additional(Some("west".to_string()), 5); // 14, 15, 16, 17, 18

        let ids = enc.row_ids_filter(&"east", &cmp::Operator::LTE, RowIDs::Vector(vec![]));
        assert_eq!(ids, RowIDs::Vector(vec![0, 1, 2, 4, 5, 6, 7, 8]));

        let ids = enc.row_ids_filter(&"east", &cmp::Operator::LT, RowIDs::Vector(vec![]));
        assert!(ids.is_empty());

        let ids = enc.row_ids_filter(&"north", &cmp::Operator::GT, RowIDs::Vector(vec![]));
        assert_eq!(ids, RowIDs::Vector(vec![9, 10, 11, 14, 15, 16, 17, 18]));

        let ids = enc.row_ids_filter(&"north", &cmp::Operator::GTE, RowIDs::Vector(vec![]));
        assert_eq!(
            ids,
            RowIDs::Vector(vec![3, 9, 10, 11, 12, 14, 15, 16, 17, 18])
        );

        // The encoding also supports comparisons on values that don't directly exist in the column.
        let ids = enc.row_ids_filter(&"abba", &cmp::Operator::GT, RowIDs::Vector(vec![]));
        assert_eq!(
            ids,
            RowIDs::Vector(vec![
                0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 14, 15, 16, 17, 18
            ])
        );

        let ids = enc.row_ids_filter(&"east1", &cmp::Operator::GT, RowIDs::Vector(vec![]));
        assert_eq!(
            ids,
            RowIDs::Vector(vec![3, 9, 10, 11, 12, 14, 15, 16, 17, 18])
        );

        let ids = enc.row_ids_filter(&"east1", &cmp::Operator::GTE, RowIDs::Vector(vec![]));
        assert_eq!(
            ids,
            RowIDs::Vector(vec![3, 9, 10, 11, 12, 14, 15, 16, 17, 18])
        );

        let ids = enc.row_ids_filter(&"east1", &cmp::Operator::LTE, RowIDs::Vector(vec![]));
        assert_eq!(ids, RowIDs::Vector(vec![0, 1, 2, 4, 5, 6, 7, 8]));

        let ids = enc.row_ids_filter(&"region", &cmp::Operator::LT, RowIDs::Vector(vec![]));
        assert_eq!(ids, RowIDs::Vector(vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 12]));

        let ids = enc.row_ids_filter(&"zoo", &cmp::Operator::LTE, RowIDs::Vector(vec![]));
        assert_eq!(
            ids,
            RowIDs::Vector(vec![
                0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 14, 15, 16, 17, 18
            ])
        );
    }

    #[test]
    fn row_ids_not_null() {
        let mut enc = Encoding::RLE(RLE::default());
        enc.push_additional(Some("east".to_string()), 3); // 0, 1, 2
        enc.push_additional(None, 3); // 3, 4, 5
        enc.push_additional(Some("north".to_string()), 1); // 6
        enc.push_additional(None, 2); // 7, 8
        enc.push_additional(Some("south".to_string()), 2); // 9, 10

        // essentially `WHERE value IS NULL`
        let ids = enc.row_ids_null(RowIDs::Vector(vec![]));
        assert_eq!(ids, RowIDs::Vector(vec![3, 4, 5, 7, 8]));

        // essentially `WHERE value IS NOT NULL`
        let ids = enc.row_ids_not_null(RowIDs::Vector(vec![]));
        assert_eq!(ids, RowIDs::Vector(vec![0, 1, 2, 6, 9, 10]));
    }

    #[test]
    fn value() {
        let mut drle = Encoding::RLE(RLE::default());
        drle.push_additional(Some("east".to_string()), 3); // 0, 1, 2
        drle.push_additional(Some("north".to_string()), 1); // 3
        drle.push_additional(Some("east".to_string()), 5); // 4, 5, 6, 7, 8
        drle.push_additional(Some("south".to_string()), 2); // 9, 10
        drle.push_none(); // 11

        assert_eq!(drle.value(3), Some(&"north".to_string()));
        assert_eq!(drle.value(0), Some(&"east".to_string()));
        assert_eq!(drle.value(10), Some(&"south".to_string()));

        assert_eq!(drle.value(11), None);
        assert_eq!(drle.value(22), None);
    }

    #[test]
    fn dictionary() {
        let mut enc = Encoding::RLE(RLE::default());
        assert!(enc.dictionary().is_empty());

        enc.push_additional(Some("east".to_string()), 23);
        enc.push_additional(Some("west".to_string()), 2);
        enc.push_none();
        enc.push_additional(Some("zoo".to_string()), 1);

        assert_eq!(
            enc.dictionary(),
            &["east".to_string(), "west".to_string(), "zoo".to_string()]
        );
    }

    #[test]
    fn values() {
        let mut enc = Encoding::RLE(RLE::default());
        enc.push_additional(Some("east".to_string()), 3); // 0, 1, 2
        enc.push_additional(Some("north".to_string()), 1); // 3
        enc.push_additional(Some("east".to_string()), 5); // 4, 5, 6, 7, 8
        enc.push_additional(Some("south".to_string()), 2); // 9, 10
        enc.push_none(); // 11

        let mut dst = Vec::with_capacity(1000);
        dst = enc.values(&[0, 1, 3, 4], dst);
        assert_eq!(
            dst,
            vec![Some("east"), Some("east"), Some("north"), Some("east"),]
        );

        dst = enc.values(&[8, 10, 11], dst);
        assert_eq!(dst, vec![Some("east"), Some("south"), None]);

        assert_eq!(dst.capacity(), 1000);

        assert!(enc.values(&[1000], dst).is_empty());
    }

    #[test]
    fn all_values() {
        let mut enc = Encoding::RLE(RLE::from(vec!["hello", "zoo"]));
        enc.push_none();

        let zoo = "zoo".to_string();
        let dst = vec![Some(&zoo), Some(&zoo), Some(&zoo), Some(&zoo)];
        let got = enc.all_values(dst);

        assert_eq!(
            got,
            [Some(&"hello".to_string()), Some(&"zoo".to_string()), None]
        );
        assert_eq!(got.capacity(), 4);
    }

    #[test]
    fn distinct_values() {
        let mut enc = Encoding::RLE(RLE::default());
        enc.push_additional(Some("east".to_string()), 100);

        let values = enc.distinct_values((0..100).collect::<Vec<_>>().as_slice(), BTreeSet::new());
        assert_eq!(
            values,
            vec![Some(&"east".to_string())]
                .into_iter()
                .collect::<BTreeSet<_>>()
        );

        enc = Encoding::RLE(RLE::default());
        enc.push_additional(Some("east".to_string()), 3); // 0, 1, 2
        enc.push_additional(Some("north".to_string()), 1); // 3
        enc.push_additional(Some("east".to_string()), 5); // 4, 5, 6, 7, 8
        enc.push_additional(Some("south".to_string()), 2); // 9, 10
        enc.push_none(); // 11

        let values = enc.distinct_values((0..12).collect::<Vec<_>>().as_slice(), BTreeSet::new());
        assert_eq!(
            values,
            vec![
                None,
                Some(&"east".to_string()),
                Some(&"north".to_string()),
                Some(&"south".to_string()),
            ]
            .into_iter()
            .collect::<BTreeSet<_>>()
        );

        let values = enc.distinct_values((0..4).collect::<Vec<_>>().as_slice(), BTreeSet::new());
        assert_eq!(
            values,
            vec![Some(&"east".to_string()), Some(&"north".to_string()),]
                .into_iter()
                .collect::<BTreeSet<_>>()
        );

        let values = enc.distinct_values(&[3, 10], BTreeSet::new());
        assert_eq!(
            values,
            vec![Some(&"north".to_string()), Some(&"south".to_string()),]
                .into_iter()
                .collect::<BTreeSet<_>>()
        );

        let values = enc.distinct_values(&[100], BTreeSet::new());
        assert!(values.is_empty());
    }

    #[test]
    fn contains_other_values() {
        let mut enc = Encoding::RLE(RLE::default());
        enc.push_additional(Some("east".to_string()), 3); // 0, 1, 2
        enc.push_additional(Some("north".to_string()), 1); // 3
        enc.push_additional(Some("east".to_string()), 5); // 4, 5, 6, 7, 8
        enc.push_additional(Some("south".to_string()), 2); // 9, 10
        enc.push_none(); // 11

        let east = &"east".to_string();
        let north = &"north".to_string();
        let south = &"south".to_string();

        let mut others = BTreeSet::new();
        others.insert(Some(east));
        others.insert(Some(north));

        assert!(enc.contains_other_values(&others));

        let f1 = "foo".to_string();
        others.insert(Some(&f1));
        assert!(enc.contains_other_values(&others));

        others.insert(Some(&south));
        others.insert(None);
        assert!(!enc.contains_other_values(&others));

        let f2 = "bar".to_string();
        others.insert(Some(&f2));
        assert!(!enc.contains_other_values(&others));

        assert!(enc.contains_other_values(&BTreeSet::new()));
    }

    #[test]
    fn has_non_null_value() {
        let mut enc = Encoding::RLE(RLE::default());
        enc.push_additional(Some("east".to_string()), 3); // 0, 1, 2
        enc.push_additional(Some("north".to_string()), 1); // 3
        enc.push_additional(Some("east".to_string()), 5); // 4, 5, 6, 7, 8
        enc.push_additional(Some("south".to_string()), 2); // 9, 10
        enc.push_none(); // 11

        assert!(enc.has_non_null_value(&[0]));
        assert!(enc.has_non_null_value(&[0, 1, 2]));
        assert!(enc.has_non_null_value(&[10]));

        assert!(!enc.has_non_null_value(&[11]));
        assert!(!enc.has_non_null_value(&[11, 12, 100]));

        // Pure NULL column...
        enc = Encoding::RLE(RLE::default());
        enc.push_additional(None, 10);
        assert!(!enc.has_non_null_value(&[0]));
        assert!(!enc.has_non_null_value(&[4, 7]));
    }

    #[test]
    fn encoded_values() {
        let mut enc = Encoding::RLE(RLE::default());
        enc.push_additional(Some("east".to_string()), 3); // 0, 1, 2
        enc.push_additional(Some("north".to_string()), 1); // 3
        enc.push_additional(Some("east".to_string()), 5); // 4, 5, 6, 7, 8
        enc.push_additional(Some("south".to_string()), 2); // 9, 10
        enc.push_none(); // 11

        let mut encoded = enc.encoded_values(&[0], vec![]);
        assert_eq!(encoded, vec![1]);

        encoded = enc.encoded_values(&[1, 3, 5, 6], vec![]);
        assert_eq!(encoded, vec![1, 2, 1, 1]);

        encoded = enc.encoded_values(&[9, 10, 11], vec![]);
        assert_eq!(encoded, vec![3, 3, 0]);
    }

    #[test]
    fn all_encoded_values() {
        let mut enc = Encoding::RLE(RLE::default());
        enc.push_additional(Some("east".to_string()), 3);
        enc.push_additional(None, 2);
        enc.push_additional(Some("north".to_string()), 2);

        let dst = Vec::with_capacity(100);
        let dst = enc.all_encoded_values(dst);
        assert_eq!(dst, vec![1, 1, 1, 0, 0, 2, 2]);
        assert_eq!(dst.capacity(), 100);
    }

    #[test]
    fn min() {
        let mut enc = Encoding::RLE(RLE::default());
        enc.push_additional(Some("east".to_string()), 3); // 0, 1, 2
        enc.push_additional(None, 2); // 3, 4
        enc.push_additional(Some("north".to_string()), 2); // 5, 6

        assert_eq!(enc.min(&[0, 1, 2]), Some(&"east".to_string()));
        assert_eq!(enc.min(&[0, 1, 2, 3, 4, 5, 6]), Some(&"east".to_string()));
        assert_eq!(enc.min(&[4, 5, 6]), Some(&"north".to_string()));
        assert_eq!(enc.min(&[3]), None);
        assert_eq!(enc.min(&[3, 4]), None);

        let mut drle = Encoding::RLE(RLE::default());
        drle.push_additional(None, 10);
        assert_eq!(drle.min(&[2, 3, 6, 8]), None);
    }

    #[test]
    fn max() {
        let mut enc = Encoding::RLE(RLE::default());
        enc.push_additional(Some("east".to_string()), 3); // 0, 1, 2
        enc.push_additional(None, 2); // 3, 4
        enc.push_additional(Some("north".to_string()), 2); // 5, 6

        assert_eq!(enc.max(&[0, 1, 2]), Some(&"east".to_string()));
        assert_eq!(enc.max(&[0, 1, 2, 3, 4, 5, 6]), Some(&"north".to_string()));
        assert_eq!(enc.max(&[4, 5, 6]), Some(&"north".to_string()));
        assert_eq!(enc.max(&[3]), None);
        assert_eq!(enc.max(&[3, 4]), None);

        let drle = Encoding::RLE(RLE::default());
        assert_eq!(drle.max(&[0]), None);

        let mut drle = Encoding::RLE(RLE::default());
        drle.push_additional(None, 10);
        assert_eq!(drle.max(&[2, 3, 6, 8]), None);
    }

    #[test]
    fn count() {
        let mut enc = Encoding::RLE(RLE::default());
        enc.push_additional(Some("east".to_string()), 3); // 0, 1, 2
        enc.push_additional(None, 2); // 3, 4
        enc.push_additional(Some("north".to_string()), 2); // 5, 6

        assert_eq!(enc.count(&[0, 1, 2]), 3);
        assert_eq!(enc.count(&[0, 1, 2, 3, 4, 5, 6]), 5);
        assert_eq!(enc.count(&[4, 5, 6]), 2);
        assert_eq!(enc.count(&[3]), 0);
        assert_eq!(enc.count(&[3, 4]), 0);

        let mut drle = Encoding::RLE(RLE::default());
        drle.push_additional(None, 10);
        assert_eq!(drle.count(&[2, 3, 6, 8]), 0);
    }
}