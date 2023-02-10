use std::ops::{Range, RangeInclusive};

pub trait Combinations {
    type Inner;

    fn combinations(&self) -> Vec<Self::Inner>;
}

macro_rules! scalar_combinations_copy {
    ($t:ty) => {
        impl Combinations for $t {
            type Inner = $t;
            fn combinations(&self) -> Vec<Self::Inner> {
                vec![*self]
            }
        }
    };
}

macro_rules! scalar_combinations_clone {
    ($t:ty) => {
        impl Combinations for $t {
            type Inner = $t;
            fn combinations(&self) -> Vec<Self::Inner> {
                vec![self.clone()]
            }
        }
    };
}

// numbers
scalar_combinations_copy!(usize);
scalar_combinations_copy!(u8);
scalar_combinations_copy!(u16);
scalar_combinations_copy!(u32);
scalar_combinations_copy!(u64);
scalar_combinations_copy!(u128);
scalar_combinations_copy!(isize);
scalar_combinations_copy!(i8);
scalar_combinations_copy!(i16);
scalar_combinations_copy!(i32);
scalar_combinations_copy!(i64);
scalar_combinations_copy!(i128);
scalar_combinations_copy!(f32);
scalar_combinations_copy!(f64);

// others
scalar_combinations_copy!(char);
scalar_combinations_copy!(bool);
scalar_combinations_copy!(());
scalar_combinations_clone!(String);

impl<T> Combinations for Range<T>
where
    Range<T>: IntoIterator<Item = T>,
    T: Clone,
{
    type Inner = T;
    fn combinations(&self) -> Vec<Self::Inner> {
        self.clone().into_iter().collect()
    }
}

impl<T> Combinations for RangeInclusive<T>
where
    RangeInclusive<T>: IntoIterator<Item = T>,
    T: Clone,
{
    type Inner = T;
    fn combinations(&self) -> Vec<Self::Inner> {
        self.clone().into_iter().collect()
    }
}

impl<T: Combinations> Combinations for Vec<T> {
    type Inner = T::Inner;
    fn combinations(&self) -> Vec<Self::Inner> {
        self.iter().flat_map(|t| t.combinations()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use expect_test::expect;
    use expect_test::Expect;

    fn check<T: Combinations + std::fmt::Debug>(val: T, expected: Expect)
    where
        <T as Combinations>::Inner: std::fmt::Debug,
    {
        expected.assert_debug_eq(&val.combinations())
    }

    #[test]
    fn test_bool() {
        check(
            false,
            expect![[r#"
                [
                    false,
                ]
            "#]],
        );
    }

    #[test]
    fn test_range() {
        check(
            0..4,
            expect![[r#"
            [
                0,
                1,
                2,
                3,
            ]
        "#]],
        );
    }

    #[test]
    fn test_range_inclusive() {
        check(
            0..=4,
            expect![[r#"
            [
                0,
                1,
                2,
                3,
                4,
            ]
        "#]],
        );
    }

    #[test]
    fn test_vec() {
        check(
            vec![1, 7, 34],
            expect![[r#"
                [
                    1,
                    7,
                    34,
                ]
            "#]],
        );
    }

    #[test]
    fn test_nested_vec() {
        check(
            vec![vec![2, 5], vec![9, 0], vec![2, 5]],
            expect![[r#"
                [
                    2,
                    5,
                    9,
                    0,
                    2,
                    5,
                ]
            "#]],
        );
    }
}
