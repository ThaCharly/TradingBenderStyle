//! Serialización canónica del estado del núcleo (doc 05 §10): el mismo estado produce
//! siempre los mismos bytes, en cualquier máquina.
//!
//! Formato: enteros big-endian de ancho fijo, `bool` como un byte, textos y bytes con
//! largo `u64` adelante, `Option` con un byte de presencia, secuencias con su largo, y los
//! mapas siempre en orden de clave (sólo `BTreeMap`; nada de `HashMap`). Los cachés que se
//! pueden recalcular (p. ej. la última vela calculada de un agregador) no se serializan.

use std::collections::BTreeMap;

use bender_types::Decimal;

/// Error al leer un estado serializado.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CheckpointError {
    /// Se terminaron los bytes antes de tiempo.
    Truncated,
    /// Un valor no es válido (etiqueta desconocida, texto no UTF-8, decimal inválido…).
    Invalid(String),
    /// Sobran bytes después del estado.
    TrailingBytes(usize),
    /// El hash guardado no coincide con el contenido: el checkpoint está corrupto.
    Corrupt { expected: String, actual: String },
}

impl std::fmt::Display for CheckpointError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Truncated => write!(f, "checkpoint truncado"),
            Self::Invalid(why) => write!(f, "checkpoint inválido: {why}"),
            Self::TrailingBytes(n) => write!(f, "sobran {n} bytes en el checkpoint"),
            Self::Corrupt { expected, actual } => {
                write!(
                    f,
                    "checkpoint corrupto: hash {actual}, se esperaba {expected}"
                )
            }
        }
    }
}

impl std::error::Error for CheckpointError {}

/// Escritor canónico.
#[derive(Default)]
pub struct Encoder {
    buf: Vec<u8>,
}

impl Encoder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.buf
    }

    pub fn put<T: Checkpointable + ?Sized>(&mut self, value: &T) -> &mut Self {
        value.encode(self);
        self
    }

    pub fn bytes(&mut self, b: &[u8]) -> &mut Self {
        self.buf.extend_from_slice(&(b.len() as u64).to_be_bytes());
        self.buf.extend_from_slice(b);
        self
    }

    fn raw(&mut self, b: &[u8]) {
        self.buf.extend_from_slice(b);
    }
}

/// Lector canónico.
pub struct Decoder<'a> {
    buf: &'a [u8],
}

impl<'a> Decoder<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        Self { buf }
    }

    pub fn get<T: Checkpointable>(&mut self) -> Result<T, CheckpointError> {
        T::decode(self)
    }

    pub fn bytes(&mut self) -> Result<&'a [u8], CheckpointError> {
        let len = usize::try_from(self.get::<u64>()?)
            .map_err(|_| CheckpointError::Invalid("largo".into()))?;
        self.take(len)
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], CheckpointError> {
        if self.buf.len() < n {
            return Err(CheckpointError::Truncated);
        }
        let (head, rest) = self.buf.split_at(n);
        self.buf = rest;
        Ok(head)
    }

    /// Falla si quedan bytes sin leer.
    pub fn finish(self) -> Result<(), CheckpointError> {
        if self.buf.is_empty() {
            Ok(())
        } else {
            Err(CheckpointError::TrailingBytes(self.buf.len()))
        }
    }
}

/// Estado que se puede serializar de forma canónica y reconstruir.
pub trait Checkpointable {
    fn encode(&self, e: &mut Encoder);
    fn decode(d: &mut Decoder<'_>) -> Result<Self, CheckpointError>
    where
        Self: Sized;
}

/// Bytes canónicos de un valor.
pub fn to_bytes<T: Checkpointable + ?Sized>(value: &T) -> Vec<u8> {
    let mut e = Encoder::new();
    value.encode(&mut e);
    e.into_bytes()
}

/// Reconstruye un valor de sus bytes canónicos (sin bytes sobrantes).
pub fn from_bytes<T: Checkpointable>(bytes: &[u8]) -> Result<T, CheckpointError> {
    let mut d = Decoder::new(bytes);
    let value = T::decode(&mut d)?;
    d.finish()?;
    Ok(value)
}

/// BLAKE3 de los bytes canónicos de un valor, en hexadecimal.
pub fn hash<T: Checkpointable + ?Sized>(value: &T) -> String {
    blake3::hash(&to_bytes(value)).to_hex().to_string()
}

macro_rules! int_impl {
    ($($t:ty),*) => {$(
        impl Checkpointable for $t {
            fn encode(&self, e: &mut Encoder) {
                e.raw(&self.to_be_bytes());
            }
            fn decode(d: &mut Decoder<'_>) -> Result<Self, CheckpointError> {
                let b = d.take(std::mem::size_of::<$t>())?;
                Ok(<$t>::from_be_bytes(b.try_into().expect("ancho fijo")))
            }
        }
    )*};
}

int_impl!(u8, u32, u64, i64, i128);

impl Checkpointable for bool {
    fn encode(&self, e: &mut Encoder) {
        e.put(&u8::from(*self));
    }
    fn decode(d: &mut Decoder<'_>) -> Result<Self, CheckpointError> {
        match d.get::<u8>()? {
            0 => Ok(false),
            1 => Ok(true),
            x => Err(CheckpointError::Invalid(format!("bool {x}"))),
        }
    }
}

impl Checkpointable for String {
    fn encode(&self, e: &mut Encoder) {
        e.bytes(self.as_bytes());
    }
    fn decode(d: &mut Decoder<'_>) -> Result<Self, CheckpointError> {
        String::from_utf8(d.bytes()?.to_vec()).map_err(|_| CheckpointError::Invalid("UTF-8".into()))
    }
}

impl<T: Checkpointable> Checkpointable for Option<T> {
    fn encode(&self, e: &mut Encoder) {
        match self {
            None => {
                e.put(&0u8);
            }
            Some(v) => {
                e.put(&1u8).put(v);
            }
        }
    }
    fn decode(d: &mut Decoder<'_>) -> Result<Self, CheckpointError> {
        match d.get::<u8>()? {
            0 => Ok(None),
            1 => Ok(Some(d.get()?)),
            x => Err(CheckpointError::Invalid(format!("Option {x}"))),
        }
    }
}

impl<T: Checkpointable> Checkpointable for Vec<T> {
    fn encode(&self, e: &mut Encoder) {
        e.put(&(self.len() as u64));
        for v in self {
            e.put(v);
        }
    }
    fn decode(d: &mut Decoder<'_>) -> Result<Self, CheckpointError> {
        let n = d.get::<u64>()?;
        // No reservar a ciegas con un largo que viene de afuera.
        let mut out = Vec::with_capacity(n.min(1 << 16) as usize);
        for _ in 0..n {
            out.push(d.get()?);
        }
        Ok(out)
    }
}

impl<K: Checkpointable + Ord, V: Checkpointable> Checkpointable for BTreeMap<K, V> {
    fn encode(&self, e: &mut Encoder) {
        e.put(&(self.len() as u64));
        for (k, v) in self {
            e.put(k).put(v);
        }
    }
    fn decode(d: &mut Decoder<'_>) -> Result<Self, CheckpointError> {
        let n = d.get::<u64>()?;
        let mut out = BTreeMap::new();
        for _ in 0..n {
            let k = d.get()?;
            let v = d.get()?;
            out.insert(k, v);
        }
        Ok(out)
    }
}

impl<A: Checkpointable, B: Checkpointable> Checkpointable for (A, B) {
    fn encode(&self, e: &mut Encoder) {
        e.put(&self.0).put(&self.1);
    }
    fn decode(d: &mut Decoder<'_>) -> Result<Self, CheckpointError> {
        Ok((d.get()?, d.get()?))
    }
}

impl<A: Checkpointable, B: Checkpointable, C: Checkpointable> Checkpointable for (A, B, C) {
    fn encode(&self, e: &mut Encoder) {
        e.put(&self.0).put(&self.1).put(&self.2);
    }
    fn decode(d: &mut Decoder<'_>) -> Result<Self, CheckpointError> {
        Ok((d.get()?, d.get()?, d.get()?))
    }
}

impl Checkpointable for Decimal {
    fn encode(&self, e: &mut Encoder) {
        e.put(&self.mantissa()).put(&self.scale());
    }
    fn decode(d: &mut Decoder<'_>) -> Result<Self, CheckpointError> {
        let mantissa: i128 = d.get()?;
        let scale: u32 = d.get()?;
        Decimal::new(mantissa, scale).map_err(|e| CheckpointError::Invalid(e.to_string()))
    }
}

/// Implementa `Checkpointable` para un struct campo por campo, en el orden dado.
#[macro_export]
macro_rules! checkpoint_struct {
    ($ty:ty { $($field:ident),* $(,)? }) => {
        impl $crate::canonical::Checkpointable for $ty {
            fn encode(&self, e: &mut $crate::canonical::Encoder) {
                $( e.put(&self.$field); )*
            }
            fn decode(d: &mut $crate::canonical::Decoder<'_>) -> Result<Self, $crate::canonical::CheckpointError> {
                Ok(Self { $( $field: d.get()?, )* })
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_fixed_layout() {
        let mut m = BTreeMap::new();
        m.insert("b".to_string(), vec![Some(1i64), None]);
        m.insert("a".to_string(), vec![]);
        let bytes = to_bytes(&m);
        assert_eq!(
            from_bytes::<BTreeMap<String, Vec<Option<i64>>>>(&bytes).unwrap(),
            m
        );
        // Largo 2, "a" primero aunque se insertó segundo.
        assert_eq!(&bytes[..8], &2u64.to_be_bytes());
        assert_eq!(&bytes[8..17], &[0, 0, 0, 0, 0, 0, 0, 1, b'a']);
        let dec: Decimal = "-12.50".parse().unwrap();
        let back: Decimal = from_bytes(&to_bytes(&dec)).unwrap();
        assert_eq!((back.mantissa(), back.scale()), (-1250, 2));
    }

    #[test]
    fn rejects_truncated_trailing_and_invalid() {
        let bytes = to_bytes(&("hola".to_string(), 7u64));
        assert_eq!(
            from_bytes::<(String, u64)>(&bytes[..bytes.len() - 1]),
            Err(CheckpointError::Truncated)
        );
        let mut extra = bytes.clone();
        extra.push(0);
        assert_eq!(
            from_bytes::<(String, u64)>(&extra),
            Err(CheckpointError::TrailingBytes(1))
        );
        assert!(matches!(
            from_bytes::<bool>(&[2]),
            Err(CheckpointError::Invalid(_))
        ));
    }
}
