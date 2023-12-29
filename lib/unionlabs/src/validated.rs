use std::{
    fmt::{Debug, Display},
    marker::PhantomData,
    ops::Deref,
    str::FromStr,
};

use either::Either;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
#[serde(
    bound(serialize = "T: Serialize", deserialize = "T: for<'d> Deserialize<'d>"),
    transparent
)]
pub struct Validated<T, V: Validate<T>>(T, #[serde(skip)] PhantomData<fn() -> V>);

pub trait ValidateT: Sized {
    fn validate<V: Validate<Self>>(self) -> Result<Validated<Self, V>, V::Error> {
        Validated::new(self)
    }
}

impl<T> ValidateT for T {}

impl<T: Debug, V: Validate<T>> Debug for Validated<T, V> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Validated").field(&self.0).finish()
    }
}

impl<T: Display, V: Validate<T>> Display for Validated<T, V> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl<T: FromStr, V: Validate<T>> FromStr for Validated<T, V> {
    type Err = Either<T::Err, V::Error>;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Validated::new(s.parse().map_err(Either::Left)?).map_err(Either::Right)
    }
}

impl<T: Clone, V: Validate<T>> Clone for Validated<T, V> {
    fn clone(&self) -> Self {
        Self(self.0.clone(), PhantomData)
    }
}

impl<T: PartialEq, V: Validate<T>> PartialEq for Validated<T, V> {
    fn eq(&self, other: &Self) -> bool {
        self.0.eq(&other.0)
    }
}

impl<T: Deref, V: Validate<T>> Deref for Validated<T, V> {
    type Target = T::Target;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

// impl<T: AsRef<U>, U, V: Validate<T> + Validate<U>> AsRef<Validated<U, V>> for Validated<T, V> {
//     fn as_ref(&self) -> &Validated<U, V> {
//         Validated(self.0.as_ref(), PhantomData)
//     }
// }

impl<T, V: Validate<T>> Validated<T, V> {
    pub fn new(t: T) -> Result<Self, V::Error> {
        V::validate(t).map(|ok| Validated(ok, PhantomData))
    }

    pub fn value(self) -> T {
        self.0
    }

    pub fn mutate<U>(
        self,
        f: impl FnOnce(T) -> U,
    ) -> Result<Validated<U, V>, <V as Validate<U>>::Error>
    where
        V: Validate<U>,
    {
        Validated::new(f(self.0))
    }
}

pub trait Validate<T>: Sized {
    type Error;

    fn validate(t: T) -> Result<T, Self::Error>;
}

impl<T, V1: Validate<T>, V2: Validate<T>> Validate<T> for (V1, V2) {
    type Error = Either<V1::Error, V2::Error>;

    fn validate(t: T) -> Result<T, Self::Error> {
        match V1::validate(t).map(|t| V2::validate(t)) {
            Ok(Ok(t)) => Ok(t),
            Ok(Err(e)) => Err(Either::Right(e)),
            Err(e) => Err(Either::Left(e)),
        }
    }
}

impl<T> Validate<T> for () {
    type Error = ();

    fn validate(t: T) -> Result<T, Self::Error> {
        Ok(t)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate() {
        #[derive(Debug, PartialEq)]
        struct NonZero;
        #[derive(Debug, PartialEq)]
        struct NonMax;
        #[derive(Debug, PartialEq)]
        struct NotEight;

        impl Validate<u8> for NonZero {
            type Error = Self;

            fn validate(t: u8) -> Result<u8, Self::Error> {
                if t == 0 {
                    Err(NonZero)
                } else {
                    Ok(t)
                }
            }
        }

        impl Validate<u8> for NonMax {
            type Error = Self;

            fn validate(t: u8) -> Result<u8, Self::Error> {
                if t == u8::MAX {
                    Err(NonMax)
                } else {
                    Ok(t)
                }
            }
        }

        impl Validate<u8> for NotEight {
            type Error = Self;

            fn validate(t: u8) -> Result<u8, Self::Error> {
                if t == 8 {
                    Err(NotEight)
                } else {
                    Ok(t)
                }
            }
        }

        assert_eq!(Validated::<_, NonZero>::new(0), Err(NonZero));

        assert_eq!(
            Validated::<_, (NonZero, ())>::new(0),
            Err(Either::Left(NonZero))
        );

        assert_eq!(
            Validated::<_, (NonZero, NonMax)>::new(0),
            Err(Either::Left(NonZero))
        );

        assert_eq!(
            Validated::<_, (NonZero, NonMax)>::new(u8::MAX),
            Err(Either::Right(NonMax))
        );

        assert_eq!(
            Validated::<_, (NonZero, NonMax)>::new(8),
            Ok(Validated(8, PhantomData))
        );

        assert_eq!(
            Validated::<_, (NonZero, (NonMax, NotEight))>::new(8),
            Err(Either::Right(Either::Right(NotEight)))
        );

        assert_eq!(
            Validated::<_, (NotEight, (NonMax, NonZero))>::new(8),
            Err(Either::Left(NotEight))
        );

        assert_eq!(
            Validated::<_, (NotEight, (NonMax, NonZero))>::new(7)
                .unwrap()
                .mutate(|t| t + 1),
            Err(Either::Left(NotEight))
        );

        assert_eq!(
            Validated::<_, (NotEight, (NonMax, NonZero))>::new(7)
                .unwrap()
                .mutate(|t| t + 2),
            Ok(Validated(9, PhantomData))
        );
    }
}
