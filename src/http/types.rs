use std::fmt::Formatter;

use serde::{Serialize, Deserialize, de::Visitor};
use sqlx::{Database, Decode, database::HasValueRef, error::BoxDynError, Type, mysql::MySqlTypeInfo, MySql};
use time::{OffsetDateTime, Format};

#[derive(sqlx::Type)]
pub struct Timestamptz(pub OffsetDateTime);

impl Serialize for Timestamptz {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.collect_str(&self.0.lazy_format(Format::Rfc3339))
    }
}

impl<'de> Deserialize<'de> for Timestamptz {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct StrVisitor;

        impl Visitor<'_> for StrVisitor {
            type Value = Timestamptz;

            fn expecting(&self, f: &mut Formatter) -> std::fmt::Result {
                f.pad("expected string")
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                OffsetDateTime::parse(v, Format::Rfc3339)
                    .map(Timestamptz)
                    .map_err(E::custom)
            }
        }

        deserializer.deserialize_str(StrVisitor)
    }
}

#[derive(serde::Serialize)]
pub struct DbBool(bool);

impl From<DbBool> for bool {
    fn from(b: DbBool) -> Self {
        b.0
    }
}

impl From<bool> for DbBool {
    fn from(b: bool) -> Self {
        Self(b)
    }
}

impl<'r, DB: Database> Decode<'r, DB> for DbBool
where
    i64: Decode<'r, DB>
{
    fn decode(value: <DB as HasValueRef<'r>>::ValueRef) -> Result<Self, BoxDynError> {
        let value = <i64 as Decode<DB>>::decode(value)?;
        Ok(DbBool(value == 1))
    }
}

impl Type<MySql> for DbBool {
    fn type_info() -> MySqlTypeInfo {
        <i64 as Type<MySql>>::type_info()
    }

    fn compatible(ty: &MySqlTypeInfo) -> bool {
        <i64 as Type<MySql>>::compatible(ty)
    }
}