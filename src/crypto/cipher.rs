// The MIT License (MIT)

// Copyright (c) 2014 Y. T. CHUNG <zonyitoo@gmail.com>

// Permission is hereby granted, free of charge, to any person obtaining a copy of
// this software and associated documentation files (the "Software"), to deal in
// the Software without restriction, including without limitation the rights to
// use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of
// the Software, and to permit persons to whom the Software is furnished to do so,
// subject to the following conditions:

// The above copyright notice and this permission notice shall be included in all
// copies or substantial portions of the Software.

// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS
// FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR
// COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER
// IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN
// CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

//! Ciphers

use std::str::FromStr;
use std::fmt::{Debug, Display, self};
use std::rand::{self, Rng};

use crypto::openssl;
use crypto::table;
#[cfg(feature = "enable-sodium")]
use crypto::sodium;
use crypto::CryptoMode;
use crypto::rc4_md5;

use crypto::digest::{self, DigestType};

/// Basic operation of Cipher, which is a Symmetric Cipher.
///
/// The `update` method could be called multiple times, and the `finalize` method will
/// encrypt the last block
pub trait Cipher {
    fn update(&mut self, data: &[u8]) -> CipherResult<Vec<u8>>;
    fn finalize(&mut self) -> CipherResult<Vec<u8>>;
}

pub type CipherResult<T> = Result<T, Error>;

#[derive(Copy)]
pub enum ErrorKind {
    OpenSSLError,
}

pub struct Error {
    pub kind: ErrorKind,
    pub desc: &'static str,
    pub detail: Option<String>,
}

impl Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        try!(write!(f, "{}", self.desc));
        match self.detail {
            Some(ref d) => write!(f, " ({})", d),
            None => Ok(())
        }
    }
}

impl Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        try!(write!(f, "{}", self.desc));
        match self.detail {
            Some(ref d) => write!(f, " ({})", d),
            None => Ok(())
        }
    }
}

#[cfg(feature = "cipher-aes-cfb")]
const CIPHER_AES_128_CFB: &'static str = "aes-128-cfb";
#[cfg(feature = "cipher-aes-cfb")]
const CIPHER_AES_128_CFB_1: &'static str = "aes-128-cfb1";
#[cfg(feature = "cipher-aes-cfb")]
const CIPHER_AES_128_CFB_8: &'static str = "aes-128-cfb8";
#[cfg(feature = "cipher-aes-cfb")]
const CIPHER_AES_128_CFB_128: &'static str = "aes-128-cfb128";

#[cfg(feature = "cipher-aes-cfb")]
const CIPHER_AES_192_CFB: &'static str = "aes-192-cfb";
#[cfg(feature = "cipher-aes-cfb")]
const CIPHER_AES_192_CFB_1: &'static str = "aes-192-cfb1";
#[cfg(feature = "cipher-aes-cfb")]
const CIPHER_AES_192_CFB_8: &'static str = "aes-192-cfb8";
#[cfg(feature = "cipher-aes-cfb")]
const CIPHER_AES_192_CFB_128: &'static str = "aes-192-cfb128";

#[cfg(feature = "cipher-aes-cfb")]
const CIPHER_AES_256_CFB: &'static str = "aes-256-cfb";
#[cfg(feature = "cipher-aes-cfb")]
const CIPHER_AES_256_CFB_1: &'static str = "aes-256-cfb1";
#[cfg(feature = "cipher-aes-cfb")]
const CIPHER_AES_256_CFB_8: &'static str = "aes-256-cfb8";
#[cfg(feature = "cipher-aes-cfb")]
const CIPHER_AES_256_CFB_128: &'static str = "aes-256-cfb128";

#[cfg(feature = "cipher-aes-ofb")]
const CIPHER_AES_128_OFB: &'static str = "aes-128-ofb";
#[cfg(feature = "cipher-aes-ofb")]
const CIPHER_AES_192_OFB: &'static str = "aes-192-ofb";
#[cfg(feature = "cipher-aes-ofb")]
const CIPHER_AES_256_OFB: &'static str = "aes-256-ofb";

#[cfg(feature = "cipher-aes-ctr")]
const CIPHER_AES_128_CTR: &'static str = "aes-128-ctr";
#[cfg(feature = "cipher-aes-ctr")]
const CIPHER_AES_192_CTR: &'static str = "aes-192-ctr";
#[cfg(feature = "cipher-aes-ctr")]
const CIPHER_AES_256_CTR: &'static str = "aes-256-ctr";

#[cfg(feature = "cipher-bf-cfb")]
const CIPHER_BF_CFB: &'static str = "bf-cfb";

#[cfg(feature = "cipher-camellia-cfb")]
const CIPHER_CAMELLIA_128_CFB: &'static str = "camellia-128-cfb";
#[cfg(feature = "cipher-camellia-cfb")]
const CIPHER_CAMELLIA_192_CFB: &'static str = "camellia-192-cfb";
#[cfg(feature = "cipher-camellia-cfb")]
const CIPHER_CAMELLIA_256_CFB: &'static str = "camellia-256-cfb";

#[cfg(feature = "cipher-cast5-cfb")]
const CIPHER_CAST5_CFB: &'static str = "cast5-cfb";
#[cfg(feature = "cipher-des-cfb")]
const CIPHER_DES_CFB: &'static str = "des-cfb";
#[cfg(feature = "cipher-idea-cfb")]
const CIPHER_IDEA_CFB: &'static str = "idea-cfb";
#[cfg(feature = "cipher-rc2-cfb")]
const CIPHER_RC2_CFB: &'static str = "rc2-cfb";
#[cfg(feature = "cipher-rc4")]
const CIPHER_RC4: &'static str = "rc4";
#[cfg(feature = "cipher-rc4")]
const CIPHER_RC4_MD5: &'static str = "rc4-md5";
#[cfg(feature = "cipher-seed-cfb")]
const CIPHER_SEED_CFB: &'static str = "seed-cfb";

const CIPHER_TABLE: &'static str = "table";

#[cfg(feature = "cipher-chacha20")]
const CIPHER_CHACHA20: &'static str = "chacha20";
#[cfg(feature = "cipher-salsa20")]
const CIPHER_SALSA20: &'static str = "salsa20";

#[derive(Clone, Debug, Copy)]
pub enum CipherType {
    Table,

    #[cfg(feature = "cipher-aes-cfb")] Aes128Cfb,
    #[cfg(feature = "cipher-aes-cfb")] Aes128Cfb1,
    #[cfg(feature = "cipher-aes-cfb")] Aes128Cfb8,
    #[cfg(feature = "cipher-aes-cfb")] Aes128Cfb128,

    #[cfg(feature = "cipher-aes-cfb")] Aes192Cfb,
    #[cfg(feature = "cipher-aes-cfb")] Aes192Cfb1,
    #[cfg(feature = "cipher-aes-cfb")] Aes192Cfb8,
    #[cfg(feature = "cipher-aes-cfb")] Aes192Cfb128,

    #[cfg(feature = "cipher-aes-cfb")] Aes256Cfb,
    #[cfg(feature = "cipher-aes-cfb")] Aes256Cfb1,
    #[cfg(feature = "cipher-aes-cfb")] Aes256Cfb8,
    #[cfg(feature = "cipher-aes-cfb")] Aes256Cfb128,

    #[cfg(feature = "cipher-aes-ofb")] Aes128Ofb,
    #[cfg(feature = "cipher-aes-ofb")] Aes192Ofb,
    #[cfg(feature = "cipher-aes-ofb")] Aes256Ofb,

    #[cfg(feature = "cipher-aes-ctr")] Aes128Ctr,
    #[cfg(feature = "cipher-aes-ctr")] Aes192Ctr,
    #[cfg(feature = "cipher-aes-ctr")] Aes256Ctr,

    #[cfg(feature = "cipher-bf-cfb")] BfCfb,

    #[cfg(feature = "cipher-camellia-cfb")] Camellia128Cfb,
    #[cfg(feature = "cipher-camellia-cfb")] Camellia192Cfb,
    #[cfg(feature = "cipher-camellia-cfb")] Camellia256Cfb,

    #[cfg(feature = "cipher-cast5-cfb")] Cast5Cfb,
    #[cfg(feature = "cipher-des-cfb")] DesCfb,
    #[cfg(feature = "cipher-idea-cfb")] IdeaCfb,
    #[cfg(feature = "cipher-rc2-cfb")] Rc2Cfb,
    #[cfg(feature = "cipher-rc4")] Rc4,
    #[cfg(feature = "cipher-rc4")] Rc4Md5,
    #[cfg(feature = "cipher-seed-cfb")] SeedCfb,

    #[cfg(feature = "cipher-chacha20")] ChaCha20,
    #[cfg(feature = "cipher-salsa20")] Salsa20,
}

impl CipherType {
    pub fn block_size(&self) -> usize {
        use libsodium_ffi::{crypto_stream_chacha20_NONCEBYTES, crypto_stream_salsa20_NONCEBYTES};

        match *self {
            CipherType::Table => 0,

            #[cfg(feature = "cipher-aes-cfb")] CipherType::Aes128Cfb => 16,
            #[cfg(feature = "cipher-aes-cfb")] CipherType::Aes128Cfb1 => 16,
            #[cfg(feature = "cipher-aes-cfb")] CipherType::Aes128Cfb8 => 16,
            #[cfg(feature = "cipher-aes-cfb")] CipherType::Aes128Cfb128 => 16,

            #[cfg(feature = "cipher-aes-cfb")] CipherType::Aes192Cfb => 16,
            #[cfg(feature = "cipher-aes-cfb")] CipherType::Aes192Cfb1 => 16,
            #[cfg(feature = "cipher-aes-cfb")] CipherType::Aes192Cfb8 => 16,
            #[cfg(feature = "cipher-aes-cfb")] CipherType::Aes192Cfb128 => 16,

            #[cfg(feature = "cipher-aes-cfb")] CipherType::Aes256Cfb => 16,
            #[cfg(feature = "cipher-aes-cfb")] CipherType::Aes256Cfb1 => 16,
            #[cfg(feature = "cipher-aes-cfb")] CipherType::Aes256Cfb8 => 16,
            #[cfg(feature = "cipher-aes-cfb")] CipherType::Aes256Cfb128 => 16,

            #[cfg(feature = "cipher-aes-ofb")] CipherType::Aes128Ofb => 16,
            #[cfg(feature = "cipher-aes-ofb")] CipherType::Aes192Ofb => 16,
            #[cfg(feature = "cipher-aes-ofb")] CipherType::Aes256Ofb => 16,

            #[cfg(feature = "cipher-aes-ctr")] CipherType::Aes128Ctr => 16,
            #[cfg(feature = "cipher-aes-ctr")] CipherType::Aes192Ctr => 16,
            #[cfg(feature = "cipher-aes-ctr")] CipherType::Aes256Ctr => 16,

            #[cfg(feature = "cipher-bf-cfb")] CipherType::BfCfb => 8,

            #[cfg(feature = "cipher-camellia-cfb")] CipherType::Camellia128Cfb => 16,
            #[cfg(feature = "cipher-camellia-cfb")] CipherType::Camellia192Cfb => 16,
            #[cfg(feature = "cipher-camellia-cfb")] CipherType::Camellia256Cfb => 16,

            #[cfg(feature = "cipher-cast5-cfb")] CipherType::Cast5Cfb => 8,
            #[cfg(feature = "cipher-des-cfb")] CipherType::DesCfb => 8,
            #[cfg(feature = "cipher-idea-cfb")] CipherType::IdeaCfb => 8,
            #[cfg(feature = "cipher-rc2-cfb")] CipherType::Rc2Cfb => 8,
            #[cfg(feature = "cipher-rc4")] CipherType::Rc4 => 0,
            #[cfg(feature = "cipher-rc4")] CipherType::Rc4Md5 => 16,
            #[cfg(feature = "cipher-seed-cfb")] CipherType::SeedCfb => 16,

            #[cfg(feature = "cipher-chacha20")] CipherType::ChaCha20 => crypto_stream_chacha20_NONCEBYTES as usize,
            #[cfg(feature = "cipher-salsa20")] CipherType::Salsa20 => crypto_stream_salsa20_NONCEBYTES as usize,
        }
    }

    pub fn key_size(&self) -> usize {
        use libsodium_ffi::{crypto_stream_chacha20_KEYBYTES, crypto_stream_salsa20_KEYBYTES};

        match *self {
            CipherType::Table => 0,

            #[cfg(feature = "cipher-aes-cfb")] CipherType::Aes128Cfb => 16,
            #[cfg(feature = "cipher-aes-cfb")] CipherType::Aes128Cfb1 => 16,
            #[cfg(feature = "cipher-aes-cfb")] CipherType::Aes128Cfb8 => 16,
            #[cfg(feature = "cipher-aes-cfb")] CipherType::Aes128Cfb128 => 16,

            #[cfg(feature = "cipher-aes-cfb")] CipherType::Aes192Cfb => 24,
            #[cfg(feature = "cipher-aes-cfb")] CipherType::Aes192Cfb1 => 24,
            #[cfg(feature = "cipher-aes-cfb")] CipherType::Aes192Cfb8 => 24,
            #[cfg(feature = "cipher-aes-cfb")] CipherType::Aes192Cfb128 => 24,

            #[cfg(feature = "cipher-aes-cfb")] CipherType::Aes256Cfb => 32,
            #[cfg(feature = "cipher-aes-cfb")] CipherType::Aes256Cfb1 => 32,
            #[cfg(feature = "cipher-aes-cfb")] CipherType::Aes256Cfb8 => 32,
            #[cfg(feature = "cipher-aes-cfb")] CipherType::Aes256Cfb128 => 32,

            #[cfg(feature = "cipher-aes-ofb")] CipherType::Aes128Ofb => 16,
            #[cfg(feature = "cipher-aes-ofb")] CipherType::Aes192Ofb => 24,
            #[cfg(feature = "cipher-aes-ofb")] CipherType::Aes256Ofb => 32,

            #[cfg(feature = "cipher-aes-ctr")] CipherType::Aes128Ctr => 16,
            #[cfg(feature = "cipher-aes-ctr")] CipherType::Aes192Ctr => 24,
            #[cfg(feature = "cipher-aes-ctr")] CipherType::Aes256Ctr => 32,

            #[cfg(feature = "cipher-bf-cfb")] CipherType::BfCfb => 16,

            #[cfg(feature = "cipher-camellia-cfb")] CipherType::Camellia128Cfb => 16,
            #[cfg(feature = "cipher-camellia-cfb")] CipherType::Camellia192Cfb => 24,
            #[cfg(feature = "cipher-camellia-cfb")] CipherType::Camellia256Cfb => 32,

            #[cfg(feature = "cipher-cast5-cfb")] CipherType::Cast5Cfb => 16,
            #[cfg(feature = "cipher-des-cfb")] CipherType::DesCfb => 8,
            #[cfg(feature = "cipher-idea-cfb")] CipherType::IdeaCfb => 16,
            #[cfg(feature = "cipher-rc2-cfb")] CipherType::Rc2Cfb => 16,
            #[cfg(feature = "cipher-rc4")] CipherType::Rc4 => 16,
            #[cfg(feature = "cipher-rc4")] CipherType::Rc4Md5 => 16,
            #[cfg(feature = "cipher-seed-cfb")] CipherType::SeedCfb => 16,

            #[cfg(feature = "cipher-chacha20")] CipherType::ChaCha20 => crypto_stream_chacha20_KEYBYTES as usize,
            #[cfg(feature = "cipher-salsa20")] CipherType::Salsa20 => crypto_stream_salsa20_KEYBYTES as usize,
        }
    }

    pub fn bytes_to_key(&self, key: &[u8]) -> Vec<u8> {
        let iv_len = self.block_size();
        let key_len = self.key_size();

        let mut m: Vec<Vec<u8>> = Vec::with_capacity((key_len + iv_len) / DigestType::Md5.digest_len() + 1);
        let mut i = 0;
        while m.len() * DigestType::Md5.digest_len() < (key_len + iv_len) {
            let mut md5 = digest::with_type(DigestType::Md5);
            if i > 0 {
                let mut vkey = m[i - 1].clone();
                vkey.push_all(key);
                md5.update(vkey.as_slice());
            } else {
                md5.update(key);
            }

            m.push(md5.digest());
            i += 1
        }

        let whole = m.into_iter().fold(Vec::new(), |mut a, b| { a.push_all(b.as_slice()); a });
        let key = whole[0..key_len].to_vec();
        key
    }

    pub fn gen_init_vec(&self) -> Vec<u8> {
        let iv_len = self.block_size();
        let mut iv = Vec::with_capacity(iv_len);
        unsafe { iv.set_len(iv_len); }
        rand::thread_rng().fill_bytes(iv.as_mut_slice());

        iv
    }
}

impl FromStr for CipherType {
    fn from_str(s: &str) -> Option<CipherType> {
        match s {
            CIPHER_TABLE | "" => Some(CipherType::Table),
            #[cfg(feature = "cipher-aes-cfb")]
            CIPHER_AES_128_CFB =>
                Some(CipherType::Aes128Cfb),
            #[cfg(feature = "cipher-aes-cfb")]
            CIPHER_AES_128_CFB_1 =>
                Some(CipherType::Aes128Cfb1),
            #[cfg(feature = "cipher-aes-cfb")]
            CIPHER_AES_128_CFB_8 =>
                Some(CipherType::Aes128Cfb8),
            #[cfg(feature = "cipher-aes-cfb")]
            CIPHER_AES_128_CFB_128 =>
                Some(CipherType::Aes128Cfb128),

            #[cfg(feature = "cipher-aes-cfb")]
            CIPHER_AES_192_CFB =>
                Some(CipherType::Aes192Cfb),
            #[cfg(feature = "cipher-aes-cfb")]
            CIPHER_AES_192_CFB_1 =>
                Some(CipherType::Aes192Cfb1),
            #[cfg(feature = "cipher-aes-cfb")]
            CIPHER_AES_192_CFB_8 =>
                Some(CipherType::Aes192Cfb8),
            #[cfg(feature = "cipher-aes-cfb")]
            CIPHER_AES_192_CFB_128 =>
                Some(CipherType::Aes192Cfb128),

            #[cfg(feature = "cipher-aes-cfb")]
            CIPHER_AES_256_CFB =>
                Some(CipherType::Aes256Cfb),
            #[cfg(feature = "cipher-aes-cfb")]
            CIPHER_AES_256_CFB_1 =>
                Some(CipherType::Aes256Cfb1),
            #[cfg(feature = "cipher-aes-cfb")]
            CIPHER_AES_256_CFB_8 =>
                Some(CipherType::Aes256Cfb8),
            #[cfg(feature = "cipher-aes-cfb")]
            CIPHER_AES_256_CFB_128 =>
                Some(CipherType::Aes256Cfb128),

            #[cfg(feature = "cipher-aes-ofb")]
            CIPHER_AES_128_OFB =>
                Some(CipherType::Aes128Ofb),
            #[cfg(feature = "cipher-aes-ofb")]
            CIPHER_AES_192_OFB =>
                Some(CipherType::Aes192Ofb),
            #[cfg(feature = "cipher-aes-ofb")]
            CIPHER_AES_256_OFB =>
                Some(CipherType::Aes256Ofb),

            #[cfg(feature = "cipher-aes-ctr")]
            CIPHER_AES_128_CTR =>
                Some(CipherType::Aes128Ctr),
            #[cfg(feature = "cipher-aes-ctr")]
            CIPHER_AES_192_CTR =>
                Some(CipherType::Aes192Ctr),
            #[cfg(feature = "cipher-aes-ctr")]
            CIPHER_AES_256_CTR =>
                Some(CipherType::Aes256Ctr),

            #[cfg(feature = "cipher-bf-cfb")]
            CIPHER_BF_CFB =>
                Some(CipherType::BfCfb),

            #[cfg(feature = "cipher-camellia-cfb")]
            CIPHER_CAMELLIA_128_CFB =>
                Some(CipherType::Camellia128Cfb),
            #[cfg(feature = "cipher-camellia-cfb")]
            CIPHER_CAMELLIA_192_CFB =>
                Some(CipherType::Camellia192Cfb),
            #[cfg(feature = "cipher-camellia-cfb")]
            CIPHER_CAMELLIA_256_CFB =>
                Some(CipherType::Camellia256Cfb),

            #[cfg(feature = "cipher-cast5-cfb")]
            CIPHER_CAST5_CFB =>
                Some(CipherType::Cast5Cfb),
            #[cfg(feature = "cipher-des-cfb")]
            CIPHER_DES_CFB =>
                Some(CipherType::DesCfb),
            #[cfg(feature = "cipher-idea-cfb")]
            CIPHER_IDEA_CFB =>
                Some(CipherType::IdeaCfb),
            #[cfg(feature = "cipher-rc2-cfb")]
            CIPHER_RC2_CFB =>
                Some(CipherType::Rc2Cfb),
            #[cfg(feature = "cipher-rc4")]
            CIPHER_RC4 =>
                Some(CipherType::Rc4),
            #[cfg(feature = "cipher-rc4")]
            CIPHER_RC4_MD5 =>
                Some(CipherType::Rc4Md5),
            #[cfg(feature = "cipher-seed-cfb")]
            CIPHER_SEED_CFB =>
                Some(CipherType::SeedCfb),

            #[cfg(feature = "cipher-chacha20")]
            CIPHER_CHACHA20 =>
                Some(CipherType::ChaCha20),
            #[cfg(feature = "cipher-salsa20")]
            CIPHER_SALSA20 =>
                Some(CipherType::Salsa20),

            _ => None
        }
    }
}

/// Generate a specific Cipher with key and initialize vector
pub fn with_type(t: CipherType, key: &[u8], iv: &[u8], mode: CryptoMode) -> Box<Cipher + Send> {
    match t {
        CipherType::Table => box table::TableCipher::new(key, mode) as Box<Cipher + Send>,

        #[cfg(feature = "cipher-chacha20")]
        CipherType::ChaCha20 =>
            box sodium::SodiumCipher::new(t, key, iv) as Box<Cipher + Send>,
        #[cfg(feature = "cipher-salsa20")]
        CipherType::Salsa20 =>
            box sodium::SodiumCipher::new(t, key, iv) as Box<Cipher + Send>,

        #[cfg(feature = "cipher-rc4")]
        CipherType::Rc4Md5 =>
            box rc4_md5::Rc4Md5Cipher::new(key, iv, mode) as Box<Cipher + Send>,

        _ => box openssl::OpenSSLCipher::new(t, key, iv, mode) as Box<Cipher + Send>,
    }
}

#[cfg(test)]
mod test_cipher {
    use crypto::cipher::{Cipher, CipherType, with_type};
    use crypto::CryptoMode;

    #[test]
    fn test_get_cipher() {
        let key = CipherType::Aes128Cfb.bytes_to_key(b"PassWORD");
        let iv = CipherType::Aes128Cfb.gen_init_vec();
        let mut encryptor = with_type(CipherType::Aes128Cfb, &key[0..], &iv[0..], CryptoMode::Encrypt);
        let mut decryptor = with_type(CipherType::Aes128Cfb, &key[0..], &iv[0..], CryptoMode::Decrypt);
        let message = "HELLO WORLD";

        let encrypted_msg = encryptor.update(message.as_bytes()).unwrap();
        let decrypted_msg = decryptor.update(encrypted_msg.as_slice()).unwrap();

        assert!(message.as_bytes() == decrypted_msg.as_slice());
    }
}
