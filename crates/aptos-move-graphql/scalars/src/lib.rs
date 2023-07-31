// Copyright © Aptos Foundation
// SPDX-License-Identifier: Apache-2.0

// todo improve this message
//! This file contains types that correspond to the scalars used in the other crates.

use move_core_types::account_address::AccountAddress;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

pub type U8 = u8;
pub type U16 = u16;
pub type U32 = u32;
pub type Address = AccountAddress;
pub type Any = serde_json::Value;

// We encode u64, u128, and u256 as strings. These types accept them as strings but
// represent them internally as actual number types.

macro_rules! define_integer_type {
    ($n:ident, $t:ty, $d:literal) => {
        #[doc = $d]
        #[doc = "Encoded as a string to encode into JSON"]
        #[derive(Clone, Debug, Default, Eq, PartialEq, Copy)]
        pub struct $n(pub $t);

        impl $n {
            pub fn inner(&self) -> &$t {
                &self.0
            }
        }

        impl From<$t> for $n {
            fn from(d: $t) -> Self {
                Self(d)
            }
        }

        impl From<$n> for $t {
            fn from(d: $n) -> Self {
                d.0
            }
        }

        impl std::fmt::Display for $n {
            fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                write!(f, "{}", &self.0)
            }
        }

        impl Serialize for $n {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                self.0.to_string().serialize(serializer)
            }
        }

        impl<'de> Deserialize<'de> for $n {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let s = <String>::deserialize(deserializer)?;
                s.parse().map_err(serde::de::Error::custom)
            }
        }

        impl std::str::FromStr for $n {
            type Err = anyhow::Error;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                let data = s.parse::<$t>().map_err(|e| {
                    anyhow::format_err!(
                        "Parsing {} string {:?} failed, caused by error: {}",
                        stringify!($t),
                        s,
                        e
                    )
                })?;

                Ok($n(data))
            }
        }
    };
}

define_integer_type!(U64, u64, "A string encoded U64.");
define_integer_type!(U128, u128, "A string encoded U128.");
define_integer_type!(U256, move_core_types::u256::U256, "A string encoded U256.");
