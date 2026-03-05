    use serde::{self, Deserialize, Deserializer};

    pub fn option_i64<'de, D: Deserializer<'de>>(d: D) -> Result<Option<i64>, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum V {
            N(i64),
            S(String),
        }
        match Option::<V>::deserialize(d)? {
            None => Ok(None),
            Some(V::N(n)) => Ok(Some(n)),
            Some(V::S(s)) => s
                .trim()
                .parse()
                .map(Some)
                .map_err(serde::de::Error::custom),
        }
    }

    pub fn option_bool<'de, D: Deserializer<'de>>(d: D) -> Result<Option<bool>, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum V {
            B(bool),
            S(String),
        }
        match Option::<V>::deserialize(d)? {
            None => Ok(None),
            Some(V::B(b)) => Ok(Some(b)),
            Some(V::S(s)) => match s.as_str() {
                "true" | "1" => Ok(Some(true)),
                "false" | "0" => Ok(Some(false)),
                _ => Err(serde::de::Error::custom(format!("invalid bool: {s}"))),
            },
        }
    }
