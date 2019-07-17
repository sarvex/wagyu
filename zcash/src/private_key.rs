use crate::address::{ZcashAddress, Format};
use crate::network::Network;
use crate::public_key::ZcashPublicKey;
use wagu_model::{Address, AddressError, PrivateKey, PrivateKeyError, PublicKey, crypto::checksum};

use base58::{FromBase58, ToBase58};
use pairing::bls12_381::Bls12;
use rand::Rng;
use rand::rngs::OsRng;
use secp256k1::Secp256k1;
use secp256k1;
use std::cmp::{Eq, PartialEq};
use std::{fmt, fmt::Debug, fmt::Display};
use std::str::FromStr;
use zcash_primitives::keys::ExpandedSpendingKey;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct P2PKHSpendingKey {
    /// The ECDSA private key
    pub secret_key: secp256k1::SecretKey,
    /// If true, the private key is serialized in compressed form
    pub compressed: bool,
    /// The network of the private key
    pub network: Network
}

impl Display for P2PKHSpendingKey {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        /// Returns a WIF string given a secp256k1 secret key.
        fn secret_key_to_wif(
            secret_key: &secp256k1::SecretKey,
            network: &Network,
            compressed: bool
        ) -> String {
            let mut wif = [0u8; 38];
            wif[0] = network.to_wif_prefix();
            wif[1..33].copy_from_slice(&secret_key[..]);

            if compressed {
                wif[33] = 0x01;
                let sum = &checksum(&wif[0..34])[0..4];
                wif[34..].copy_from_slice(sum);
                wif.to_base58()
            } else {
                let sum = &checksum(&wif[0..33])[0..4];
                wif[33..37].copy_from_slice(sum);
                wif[..37].to_base58()
            }
        }
        write!(f, "{}", secret_key_to_wif(&self.secret_key, &self.network, self.compressed))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct P2SHSpendingKey {}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct SproutSpendingKey {}

#[derive(Clone)]
pub struct SaplingSpendingKey {
    pub(crate) spending_key: Option<[u8; 32]>,
    pub(crate) expanded_spending_key: ExpandedSpendingKey<Bls12>,
    pub(crate) network: Network
}

impl Debug for SaplingSpendingKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "SaplingSpendingKey {{ sk: {:?}, ask: {:?}, nsk: {:?}, ovk: {:?}, network: {:?} }}",
            self.spending_key,
            self.expanded_spending_key.ask,
            self.expanded_spending_key.nsk,
            self.expanded_spending_key.ovk,
            self.network)?;
        Ok(())
    }
}

impl Display for SaplingSpendingKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(spending_key) = self.spending_key {
            for s in &spending_key[..] {
                write!(f, "{:02x}", s)?;
            }
        } else {
            let mut buffer = vec![0; 96];
            match self.expanded_spending_key.write(buffer.as_mut_slice()).is_ok() {
                true => for s in &buffer[..] {
                    write!(f, "{:02x}", s)?;
                },
                false => {
                    write!(f, "unable to print expanded spending key")?;
                }
            }
        }
        Ok(())
    }
}

impl PartialEq for SaplingSpendingKey {
    fn eq(&self, other: &Self) -> bool {
        if self.network != other.network {
            return false
        }
        if self.spending_key.is_some() && other.spending_key.is_some() {
            if self.spending_key.unwrap() != other.spending_key.unwrap() {
                return false
            }
        }
        self.expanded_spending_key.ask == other.expanded_spending_key.ask
            && self.expanded_spending_key.nsk == other.expanded_spending_key.nsk
            && self.expanded_spending_key.ovk == other.expanded_spending_key.ovk
    }
}

impl Eq for SaplingSpendingKey {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpendingKey {
    /// P2PKH transparent spending key
    P2PKH(P2PKHSpendingKey),
    /// P2SH transparent spending key
    P2SH(P2SHSpendingKey),
    /// Sprout shielded spending key
    Sprout(SproutSpendingKey),
    /// Sapling shielded spending key
    Sapling(SaplingSpendingKey)
}

/// Represents a Zcash Private Key
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZcashPrivateKey(pub(crate) SpendingKey);

impl PrivateKey for ZcashPrivateKey {
    type Address = ZcashAddress;
    type Format = Format;
    type Network = Network;
    type PublicKey = ZcashPublicKey;

    /// Returns a randomly-generated compressed Zcash private key.
     fn new(network: &Network) -> Result<Self, PrivateKeyError> {
        let mut random = [0u8; 32];
        OsRng.try_fill(&mut random)?;
        Ok(Self(SpendingKey::P2PKH(P2PKHSpendingKey {
            secret_key: secp256k1::SecretKey::from_slice(&Secp256k1::new(), &random)?,
            compressed: true,
            network: *network
        })))
    }

    /// Returns the public key of the corresponding Zcash private key.
     fn to_public_key(&self) -> Self::PublicKey {
        ZcashPublicKey::from_private_key(self)
    }

    /// Returns the address of the corresponding Zcash private key.
    fn to_address(&self, format: &Self::Format) -> Result<Self::Address, AddressError> {
        ZcashAddress::from_private_key(self, format)
    }
}

impl ZcashPrivateKey {

    /// Returns either a Zcash private key struct or errors.
    pub fn from(s: &str, format: &Format, network: &Network) -> Result<Self, PrivateKeyError> {
        match format {
            Format::P2PKH => Self::p2pkh(s, network),
            Format::Sapling(_) => match hex::decode(s)?.len() {
                32 => Self::sapling(s, network),
                96 => Self::sapling_expanded(s, network),
                length => Err(PrivateKeyError::InvalidByteLength(length))
            },
            _ => Err(PrivateKeyError::UnsupportedFormat)
        }
    }

    /// Returns the network this private key is intended for.
    pub fn network(&self) -> Network {
        match &self.0 {
            SpendingKey::P2PKH(spending_key) => spending_key.network,
            SpendingKey::Sapling(spending_key) => spending_key.network,
            _ => Network::Mainnet
        }
    }

    /// Returns a P2PKH private key from a given WIF.
    fn p2pkh(wif: &str, network: &Network) -> Result<Self, PrivateKeyError> {
        let data = wif.from_base58()?;
        let len = data.len();
        if len != 37 && len != 38 {
            return Err(PrivateKeyError::InvalidCharacterLength(len))
        }

        let expected = &data[len - 4..][0..4];
        let checksum = &checksum(&data[0..len - 4])[0..4];
        if *expected != *checksum {
            let expected = expected.to_base58();
            let found = checksum.to_base58();
            return Err(PrivateKeyError::InvalidChecksum(expected, found))
        }

        if *network != Network::from_wif_prefix(data[0])? {
            let expected = network.to_string();
            let found = Network::from_wif_prefix(data[0])?.to_string();
            return Err(PrivateKeyError::InvalidNetwork(expected, found))
        }

        Ok(Self(SpendingKey::P2PKH(P2PKHSpendingKey {
            secret_key: secp256k1::SecretKey::from_slice(&Secp256k1::without_caps(), &data[1..33])?,
            compressed: len == 38,
            network: *network
        })))
    }

    /// Returns a Sapling private key from a given seed.
    fn sapling(spending_key: &str, network: &Network) -> Result<Self, PrivateKeyError> {
        let data = hex::decode(spending_key)?;
        if data.len() != 32 {
            return Err(PrivateKeyError::InvalidByteLength(data.len()));
        }

        let mut sk = [0u8; 32];
        sk.copy_from_slice(data.as_slice());

        Ok(Self(SpendingKey::Sapling(SaplingSpendingKey {
            spending_key: Some(sk),
            expanded_spending_key: ExpandedSpendingKey::from_spending_key(&sk[..]),
            network: *network
        })))
    }

    /// Returns a Sapling private key from a given expanded spending key.
    fn sapling_expanded(
        expanded_spending_key: &str,
        network: &Network
    ) -> Result<Self, PrivateKeyError> {
        let data = hex::decode(expanded_spending_key)?;
        if data.len() != 96 {
            return Err(PrivateKeyError::InvalidByteLength(data.len()));
        }

        Ok(Self(SpendingKey::Sapling(SaplingSpendingKey {
            spending_key: None,
            expanded_spending_key: ExpandedSpendingKey::read(&data[..])?,
            network: *network
        })))
    }
}

impl FromStr for ZcashPrivateKey {
    type Err = PrivateKeyError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let b58 = s.from_base58();
        let hex = hex::decode(s);

        // Transparent
        if b58.is_ok() && hex.is_err() {
            let data = b58.unwrap();
            if data.len() != 37 && data.len() != 38 {
                return Err(PrivateKeyError::InvalidByteLength(data.len()))
            }
            return Self::p2pkh(s, &Network::from_wif_prefix(data[0])?)
        }

        // Shielded
        if b58.is_err() && hex.is_ok() {
            let data = hex.unwrap();
            if data.len() == 32 {
                return Self::sapling(s, &Network::Mainnet)
            } else if data.len() == 96 {
                return Self::sapling_expanded(s, &Network::Mainnet)
            }
        }

        Err(PrivateKeyError::UnsupportedFormat)
    }
}

impl Display for ZcashPrivateKey {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match &self.0 {
            SpendingKey::P2PKH(p2pkh) => write!(f, "{}", p2pkh.to_string()),
            SpendingKey::Sapling(sapling) => write!(f, "{}", sapling.to_string()),
            _ =>  write!(f, "")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_to_public_key(expected_public_key: &ZcashPublicKey, private_key: &ZcashPrivateKey) {
        let public_key = private_key.to_public_key();
        assert_eq!(*expected_public_key, public_key);
    }

    fn test_to_address(
        expected_address: &ZcashAddress,
        expected_format: &Format,
        private_key: &ZcashPrivateKey
    ) {
        let address = private_key.to_address(expected_format).unwrap();
        assert_eq!(*expected_address, address);
    }

    fn test_from(
        expected_spending_key: &SpendingKey,
        expected_network: &Network,
        expected_public_key: &str,
        expected_address: &str,
        expected_format: &Format,
        seed: &str
    ) {
        let private_key = ZcashPrivateKey::from(seed, expected_format, expected_network).unwrap();
        assert_eq!(*expected_spending_key, private_key.0);
        assert_eq!(*expected_network, private_key.network());
        assert_eq!(expected_public_key, private_key.to_public_key().to_string());
        assert_eq!(expected_address, private_key.to_address(expected_format).unwrap().to_string());
    }

    fn test_to_str(expected_private_key: &str, private_key: &ZcashPrivateKey) {
        assert_eq!(expected_private_key, private_key.to_string());
    }

    mod p2pkh_mainnet_compressed {
        use super::*;

        const KEYPAIRS: [(&str, &str, &str); 5] = [
            (
                "L3a3yRcYATnZQt7ams14Pe5KCyRzrrCSejDyeQzHXGntToffVH4g",
                "0310d63f8c2f0a6efd13ce8a77776de26eba1816f73aa73e73a4da3f2368fcc949",
                "t1JwBjJWgNQVqWxGha2RsPZMhVGgfRg2pod"
            ),
            (
                "Kx7f3xE2TmhczSkFUxxSajE2vuuLrrqinAbTZBxqxHj6XGbhoyrQ",
                "02f4bf56c9c8389b04752236a4f2419367e3a4e36fe80da6162a0b530ca91262b0",
                "t1VnZLVwvaUsnYt34XJHNTu24wn3kD8RwsE"
            ),
            (
                "L46n9WGR671oANndbkxBBz9orQ36TQu98zeRJmp41tqk3HM6UpJk",
                "031347c183c608c629e8bc0ad76718cc9f2a1ee9e53d45862a1b9c8fad25f8ab5b",
                "t1N8HuTxFm9qS7yQCi3TsMGCQ8kPPTx5Me7"
            ),
            (
                "L2AMjT43hZQGATgtkakVMMMEguoJLwDAcZJVg1zsqjWeWaC4cTVd",
                "03a0d8ab54a080f6e085777c2f5432b22b3543ad421aecc3f2136bcd2e1e2a59e4",
                "t1PUKYyoqPZw43CHqjquU9PZE1GEvmHNbPa"
            ),
            (
                "L53GxzD5rVaX6jY5ig1qNBqur5WyAeFn8sCo9VwU4J717ewDbgc6",
                "020ceda15424ec7159f7ac5f6ad2654c93ab4cae7f9419de7aae39967f97907fd7",
                "t1TqidZPmPSJsr1wcMuYwDDaa7D9ow5sWMx"
            )
        ];

        #[test]
        fn to_public_key() {
            KEYPAIRS.iter().for_each(|(private_key, public_key, _)| {
                let public_key = ZcashPublicKey::from_str(public_key).unwrap();
                let private_key = ZcashPrivateKey::from_str(&private_key).unwrap();
                test_to_public_key(&public_key, &private_key);
            });
        }

        #[test]
        fn to_address() {
            KEYPAIRS.iter().for_each(|(private_key, _, address)| {
                let address = ZcashAddress::from_str(address).unwrap();
                let private_key = ZcashPrivateKey::from_str(&private_key).unwrap();
                test_to_address(&address, &Format::P2PKH, &private_key);
            });
        }

        #[test]
        fn from() {
            KEYPAIRS.iter().for_each(|(private_key, expected_public_key, expected_address)| {
                let expected_private_key = ZcashPrivateKey::from_str(&private_key).unwrap();
                test_from(
                    &expected_private_key.0,
                    &Network::Mainnet,
                    expected_public_key,
                    expected_address,
                    &Format::P2PKH,
                    &private_key);
            });
        }

        #[test]
        fn to_str() {
            KEYPAIRS.iter().for_each(|(expected_private_key, _, _)| {
                let private_key = ZcashPrivateKey::from_str(expected_private_key).unwrap();
                test_to_str(expected_private_key, &private_key);
            });
        }
    }

    mod p2pkh_mainnet_uncompressed {
        use super::*;

        const KEYPAIRS: [(&str, &str, &str); 5] = [
            (
                "5JkYwYTFDzd41Uy3qB8ucvENFzFBYHnZGk7GbFnHTwUaepikxpJ",
                "0471b47908e7a0cd0e053129cde9a38c54730bc63faf780efc4f9b7c3db4ed1b7db0f877ae0e1959d2353bca05bc405fa1c48e76fec3e99c26e48c95cf112dc7c9",
                "t1Rxy8Qw6eXxSRFLwS3S1D8T436eR4zQTKp"
            ),
            (
                "5JBCdxHg7w5gDNVi4G34zHWNMqvPheZAG4TeQcwk5jgh7RcenAZ",
                "049af1ad996f0ca009bc2e7bcdb4a899f822dfca068dfabf8aa7fb2be86c5c3cf198efbdfb3c870b01c81e3236e0dd4db0fe279a31695ce17cc83b94fe85d250ab",
                "t1PpkWq8MVDcpG7mneEhgmVZnkpM1vQQJdx"
            ),
            (
                "5JQjtXVXkf1trwNCPK9KsUapYDrwKUYnPZwX5zFdfy7DiFfEv2g",
                "0487dbab62116ed483bec0d8f4422e1ab315e65b8f981f6e4bd17621e393c8e7632b4028807695d959691d2e121a8e953c47e618defa6e9c159f7fdf60870981c5",
                "t1PsCpuxCMZ44j3H5tuTLmdPKqdhR3N4TPf"
            ),
            (
                "5Kht325G1JErVxAKcM7WreWR9oVgwsN21m4VVoVKiJ4vAm9mLZQ",
                "04463f48d8b3d7e622900633cd409f851c49fec6607eba3db52965995b300e8abaea439a2d5bd6f6b86a53198eac8d2735a4b013f8a811e1b151fdb5b5f11c595d",
                "t1XDKPwTpFS2BWWXASnhTLKWfoaFocRGErk"
            ),
            (
                "5KA9KkgmBfSUiRqBdsjiAjRzcQbGcYD5EpWArFfNRsw1h4bwmqU",
                "04315a06c80dd5886e960b213fdefd9df76fa2b26f9e3e876a72160a20af1fea95425c12d5c5ed75e89a564a4bdcdf5fbb197e29b2b042016987f2eaf64b26d5f3",
                "t1Zk2uJgGLZCJCRXUYUGTpRqqn3utqsdPsg"
            )
        ];

        #[test]
        fn to_public_key() {
            KEYPAIRS.iter().for_each(|(private_key, public_key, _)| {
                let public_key = ZcashPublicKey::from_str(public_key).unwrap();
                let private_key = ZcashPrivateKey::from_str(&private_key).unwrap();
                test_to_public_key(&public_key, &private_key);
            });
        }

        #[test]
        fn to_address() {
            KEYPAIRS.iter().for_each(|(private_key, _, address)| {
                let address = ZcashAddress::from_str(address).unwrap();
                let private_key = ZcashPrivateKey::from_str(&private_key).unwrap();
                test_to_address(&address, &Format::P2PKH, &private_key);
            });
        }

        #[test]
        fn from() {
            KEYPAIRS.iter().for_each(|(private_key, expected_public_key, expected_address)| {
                let expected_private_key = ZcashPrivateKey::from_str(&private_key).unwrap();
                test_from(
                    &expected_private_key.0,
                    &Network::Mainnet,
                    expected_public_key,
                    expected_address,
                    &Format::P2PKH,
                    &private_key);
            });
        }

        #[test]
        fn to_str() {
            KEYPAIRS.iter().for_each(|(expected_private_key, _, _)| {
                let private_key = ZcashPrivateKey::from_str(expected_private_key).unwrap();
                test_to_str(expected_private_key, &private_key);
            });
        }
    }

    mod p2pkh_testnet_compressed {
        use super::*;

        const KEYPAIRS: [(&str, &str, &str); 5] = [
            (
                "cNG7sM13VvGrhKgepLeEiiQAERXpGB6j5NuRwhh6sLh2skMTQf7M",
                "02d327c40e543a08c17cda94d0b9660520bd075151280e487294e94eced3a283df",
                "tmWT3bvWCHQkAXXucPjWHqLs9EyWUDdzSuN"
            ),
            (
                "cQXFXQHBzCuPbKYdeKERGeMrh8TJAsos7TLYDamQLXJiXY9sUkY6",
                "038a2754d1b25a7d0cb3518ea92ca07de0fc21a56d920be6ca10857893c48989fb",
                "tmAs578aq6jaXqmnXRrhWpjJsySFf7eXb5J"
            ),
            (
                "cQNALaabLLxMzdBkbCZvcTJtyvQ5zg4UhhskMk5R8Wu1ymSXCLsX",
                "0309341fa999f0f2951eb9867f84b55781904fe2228b8ffc8dc1a8a47e1c357957",
                "tmD8R6k2mTfTwGG24w5SBeAwQnqKGFx3cSg"
            ),
            (
                "cS6qPDRjncjCAe95SGKH81491NGkwzWqhAsGTEzkgVNC6ZdBpB4M",
                "024e12c05184403e0243a1563b9ebaeda7b529bf1306abe55827d363697be936a4",
                "tmPNWe7d4Hvkh4TEZ6Xd1ZQBje7VNQQ2Anb"
            ),
            (
                "cRRjNZuyYu8aiqVLRLvj7PqTWKLELK2N257AgSvmPzMjfJ44oWtb",
                "03d08c6748dcad37dbcad05d4cde25234107785a1c19b6edda8bfc199c91877d7d",
                "tmRdRE5JAX6KX3c11GVgqc5R6JBRtfbuk8i"
            )
        ];

        #[test]
        fn to_public_key() {
            KEYPAIRS.iter().for_each(|(private_key, public_key, _)| {
                let public_key = ZcashPublicKey::from_str(public_key).unwrap();
                let private_key = ZcashPrivateKey::from_str(&private_key).unwrap();
                test_to_public_key(&public_key, &private_key);
            });
        }

        #[test]
        fn to_address() {
            KEYPAIRS.iter().for_each(|(private_key, _, address)| {
                let address = ZcashAddress::from_str(address).unwrap();
                let private_key = ZcashPrivateKey::from_str(&private_key).unwrap();
                test_to_address(&address, &Format::P2PKH, &private_key);
            });
        }

        #[test]
        fn from() {
            KEYPAIRS.iter().for_each(|(private_key, expected_public_key, expected_address)| {
                let expected_private_key = ZcashPrivateKey::from_str(&private_key).unwrap();
                test_from(
                    &expected_private_key.0,
                    &Network::Testnet,
                    expected_public_key,
                    expected_address,
                    &Format::P2PKH,
                    &private_key);
            });
        }

        #[test]
        fn to_str() {
            KEYPAIRS.iter().for_each(|(expected_private_key, _, _)| {
                let private_key = ZcashPrivateKey::from_str(expected_private_key).unwrap();
                test_to_str(expected_private_key, &private_key);
            });
        }
    }

    mod p2pkh_testnet_uncompressed {
        use super::*;

        const KEYPAIRS: [(&str, &str, &str); 5] = [
            (
                "92VN8AQdnRoBRw7QQcpUYsh1bSEWUrkZ364fjMUdNrm8fTJhEGw",
                "04d7d44136ec02643e813d1182256508622385d18052bf13ec475ce226d161d7832cb4f7032e900ccfc07f3a4870a1522f7b4ce381bc1f8c0bcc5d36ec3c9d351a",
                "tmCrh5cZDv5KUpcmbYzoaC4SZUTTbeoERdX"
            ),
            (
                "92cPhKtH4PfSsJBihSvd5aeZXU5gvzJWM2pHVWZ4dgYtioLb5Vm",
                "0414e08f6a2fa6b0cc205aef1be0c8e87dbcc312f72c46e2446441c0e42fb51b20982678deade96a1851f4f673774fd834b8a18ccae6aef65fe53098ad533ee944",
                "tmEev5ommgX8J1E2cgygciUtB6BkAX3CYWc"
            ),
            (
                "921RfpWAirU31BCKn8LhctV3hF3EJCVGDok5jZGYhoNc5Cp8TxZ",
                "0408d8331b48fe348e7657f0cdbbd8027c715dabd62d9a94d6a028b9a5a972fc22548948c11eddb61f90a5da9a7647f4e4c2859def91ac45c1cea8f38817f9129e",
                "tmVttNTD4jkGikWYpmCmCCmtYk7s3EBTcGw"
            ),
            (
                "92nYXXcwdSZSykBUuRJUsGCeQQn1GimuDp3dfThsr1oVFft7mJR",
                "0433287b651e3df0d7fd32494673f7aaf5dabe9e4e9e9c292a4c4f2aab3b68845648a657ae274013e363ce89ab7f938ded9e3df1ef66fb0aadf7e41b823d8e3d34",
                "tmTSQ5RKyeND95C39kg8uJA9H1qD1WXvfQf"
            ),
            (
                "92t16UPRvwotFBR3CQfc9TweC5dqPiNwiaLSZLhbtGZkLM4RYpG",
                "042b9da4fe2356a93d83cf43e0c2b1714193ae9ad3ffd9cbfe3d1bd3ec6540345199ad024df690f8ffa1fe57ae675d14a510a919625aedf3442c3e2e4b43ff0683",
                "tmAhSm3UDLkKhZUAcAz4W83hmC4sgWS3zwk"
            )
        ];

        #[test]
        fn to_public_key() {
            KEYPAIRS.iter().for_each(|(private_key, public_key, _)| {
                let public_key = ZcashPublicKey::from_str(public_key).unwrap();
                let private_key = ZcashPrivateKey::from_str(&private_key).unwrap();
                test_to_public_key(&public_key, &private_key);
            });
        }

        #[test]
        fn to_address() {
            KEYPAIRS.iter().for_each(|(private_key, _, address)| {
                let address = ZcashAddress::from_str(address).unwrap();
                let private_key = ZcashPrivateKey::from_str(&private_key).unwrap();
                test_to_address(&address, &Format::P2PKH, &private_key);
            });
        }

        #[test]
        fn from() {
            KEYPAIRS.iter().for_each(|(private_key, expected_public_key, expected_address)| {
                let expected_private_key = ZcashPrivateKey::from_str(&private_key).unwrap();
                test_from(
                    &expected_private_key.0,
                    &Network::Testnet,
                    expected_public_key,
                    expected_address,
                    &Format::P2PKH,
                    &private_key);
            });
        }

        #[test]
        fn to_str() {
            KEYPAIRS.iter().for_each(|(expected_private_key, _, _)| {
                let private_key = ZcashPrivateKey::from_str(expected_private_key).unwrap();
                test_to_str(expected_private_key, &private_key);
            });
        }
    }

    mod sapling_mainnet {
        use super::*;

        const KEYPAIRS: [(&str, &str, &str); 5] = [
            (
                "bb69cdb5e70e2bbd24f771cd15a18ad58d3ab9e1aa3cab186b9b65d17f7aadef",
                "d21167e8ae8ccbcd34f96ec58bdf798ca7994217d03812100ea9e5cd4e1596ce3a3cf2b6c632c45d9da3b0c044d82655969f71652507eebf25e504486b7fb8e4afc9f1e8cf6b8eae18b786ec79d218d0a1cff90b43273ea162da99a9d2e21dff",
                "zs1dq9dlh6u6hna0u96aqtynxt3acddtgkgdx4re65500nmc2aze0my65ky36vaqvj4hkc9ut66eyf"
            ),
            (
                "7be697adb66f36d37b12dcdbdea38fbaec8340402de43bfe016f3c10b6a7220e",
                "1ac13265a8948db61e1614c50a71bc0af2fee7e9814d041b4c8a6a6a5bcfc9cc64b5eb4632e433a0eccc9db485121625345950693c90244e656faf2b1a356f0eb04548d7772c9be13301a71c497afc8d46f805ebce5066371a5548db109611e3",
                "zs1vvdj0st065ngdruymdcdy63duuavjeww3a2yyeu5tsqj2azhvwgkcaw9ngggfas6h4z4whnkpwz"
            ),
            (
                "0c9f5d70eaac46862150ae3f2a4eecc68753a72567eb66210df8e18a91425adf",
                "bb2d4d7e05b1afb686a7e4d7d8e82a592f25b26caa78a06e939e0ef835c6100c738d89a62c2acd969ef1c68d67d9d365b277145cc60a8e95e11315e192b22c29f612476ea95aa2d4b7df5b881c363829b39ccaa6318c6df3bd2ba6274a15fea0",
                "zs1akf8swew32rr4n63qedewhp2yz3wcjeazp6efs82lgealmux0h30ayju440rqyuscdr3wd5yuap"
            ),
            (
                "fc1edae9146d5c7f9398871ac09097fea6c1593e8c7b6f3384af36ff9cc3b2ee",
                "d610ec21ba084c1b4f42e9c38eefce1dfbf5c0843a549d08b5119007745b171db468b58307cd2c7e54ede334c4e98593e21776043e8956740b102513c03cb023301a9133ee59b826143304b041d8e1f2f1f91d3625ad7dac9e4c88e630a76d8d",
                "zs14q3vapgrd6wfs9pr7hfy37y9djm3gnq09ztxsqs2x2vzv0lck978843q8r2ysejgwp9mcx7ws48"
            ),
            (
                "6038f5e45498c92edd5e6a2588bcce7bcbac604e4e825ee7015d11f33d1e9673",
                "9847a15f3393ad8921f2b282e3033d48adb2eae9455f1e6b77038be913e04c51967b6db4f726fc21c57cf73d3c1fac7d044bc8ea2c5a75334f4641d18f2d0c1335c884a4853f740e93e55d7b9a7b82e7d8c6d17b2305282143359807f2d690d1",
                "zs1rzjhudlm99h5fyrh7dfsvkfg9l5z587w97pm3ce9hpwfxpgck6p55lwu5mcapz7g3r40y597n2c"
            )
        ];

        #[test]
        fn to_public_key() {
            KEYPAIRS.iter().for_each(|(private_key, public_key, _)| {
                let public_key = ZcashPublicKey::from_str(public_key).unwrap();
                let private_key = ZcashPrivateKey::from(&private_key, &Format::Sapling(None), &Network::Mainnet).unwrap();
                test_to_public_key(&public_key, &private_key);
            });
        }

        #[test]
        fn to_address() {
            KEYPAIRS.iter().for_each(|(private_key, _, address)| {
                let address = ZcashAddress::from_str(address).unwrap();
                let private_key = ZcashPrivateKey::from(&private_key, &address.format, &Network::Mainnet).unwrap();
                test_to_address(&address, &address.format, &private_key);
            });
        }

        #[test]
        fn from() {
            KEYPAIRS.iter().for_each(|(private_key, expected_public_key, expected_address)| {
                let expected_private_key = ZcashPrivateKey::from(&private_key, &Format::Sapling(None), &Network::Mainnet).unwrap();
                test_from(
                    &expected_private_key.0,
                    &Network::Mainnet,
                    expected_public_key,
                    expected_address,
                    &Format::Sapling(Some(ZcashAddress::get_diversifier(expected_address).unwrap())),
                    &private_key);
            });
        }

        #[test]
        fn to_str() {
            KEYPAIRS.iter().for_each(|(expected_private_key, _, _)| {
                let private_key = ZcashPrivateKey::from(&expected_private_key, &Format::Sapling(None), &Network::Mainnet).unwrap();
                test_to_str(expected_private_key, &private_key);
            });
        }
    }

    mod sapling_testnet {
        use super::*;

        const KEYPAIRS: [(&str, &str, &str); 5] = [
            (
                "49110debf1fac0086a2fabd60aab413d0281732b6e51a03dd6ec4f334469ef9f",
                "35d5cf61a3d8cf3078112693c1839a14307179008ea5f0902810d03e4a05d8bd194129f2b82ded4a973ad24aa3e4d8e49a10e039f5060616981511d6a888ca8eca66a812697612fc31fa4e0928ac144a3938d5793beda10f7513e15a6f95ad80",
                "ztestsapling1jzzt7gjscav7lmdpemknv0v8rmmdzpcaqrx95azrgaky94drrvf0fg4wlnlkaclqj3r3s23g2sf"
            ),
            (
                "4e9d5d14d776a93e8aa1dd7e69eda7cefd9651ad140443ca11553e379b2ae90b",
                "b8f5a0b850db6424b704e0eda9d01ef472fc58dd1519cb60a97d9240641e105e8856e062da9c729bb3580b64c5d933190af86066e518922e7967255746c024ea4bc12f2e92f93f4f853161bf774fe4d9c32581020c9cc52ee42216c8e575d4bb",
                "ztestsapling19epsvtxnzf59pr993fq4g0gu0fmrn2jl2z9jm2lgj3220c7r9shyvcpe25ul7wxvzk60z82zyf7"
            ),
            (
                "8544e9cfc6423e22bca5b62bf56649fd3716b6cc092391ecba78fb017d5feda1",
                "75b6233bd29155a361ec2a98552f5c1ddded2fd47af880d78a3f26b4ce8cc2d262008e3d2904bfeb61abdb70432860fbe6557a406c1ae72a4a204fe934985116cdf80f5b52fbc7d22c2dd630939b7641cec76b2e4f8ef6dc53276ea4b3efc1a9",
                "ztestsapling18ur694qcm6w657u9xt8aekutn98gyvpzwzjgjz99594x775ppeze5vwnp2ndw0u205vkuh2tqcu"
            ),
            (
                "6d21907f6ad14d2823625036e0951a3c566d4df7b101dfb2899107d02e9bd8bd",
                "e35af2ffa9c77482e11d998d087492e9f80672558253ef30ddb6d712d51a4aacc58455349ccd5be0de5a9584cfe63a5be7e86eb9f20f8efb3fb1ef84d8fc6625aa38f308de656736fb6b02929d695bb904108d5b680952ce774e938b3c50418b",
                "ztestsapling1hkyeldalqna6kxzkkpc3gl4yvtd842sld4kkx7mhtm4srhndnqm347q7x672t05j245skqsctvs"
            ),
            (
                "d800f2b919cb06f7396a9e253c77f65e1cb5f972372cac196ec6546e09355bfe",
                "1ba76bdbb4036d8564562e6664af996c53eebd5fd0209894d9100d6caf3fd149bb2550161ca124c672d7d5d2d7fd9fbbda1423e49585f22d269f59898c58e5bdea43b13df61ecd6cd23e6151a873e575db9b54a43e17cd5b12a406f6e9db6073",
                "ztestsapling12n4jm24lflgmjk4crm0322p0gpmww98v5cqyurphq6tr4r4q9kxyz2f3tp9x92mm8kruwwg2u5w"
            )
        ];

        #[test]
        fn to_public_key() {
            KEYPAIRS.iter().for_each(|(private_key, public_key, _)| {
                let public_key = ZcashPublicKey::from_str(public_key).unwrap();
                let private_key = ZcashPrivateKey::from(&private_key, &Format::Sapling(None), &Network::Testnet).unwrap();
                test_to_public_key(&public_key, &private_key);
            });
        }

        #[test]
        fn to_address() {
            KEYPAIRS.iter().for_each(|(private_key, _, address)| {
                let address = ZcashAddress::from_str(address).unwrap();
                let private_key = ZcashPrivateKey::from(&private_key, &address.format, &Network::Testnet).unwrap();
                test_to_address(&address, &address.format, &private_key);
            });
        }

        #[test]
        fn from() {
            KEYPAIRS.iter().for_each(|(private_key, expected_public_key, expected_address)| {
                let expected_private_key = ZcashPrivateKey::from(&private_key, &Format::Sapling(None), &Network::Testnet).unwrap();
                test_from(
                    &expected_private_key.0,
                    &Network::Testnet,
                    expected_public_key,
                    expected_address,
                    &Format::Sapling(Some(ZcashAddress::get_diversifier(expected_address).unwrap())),
                    &private_key);
            });
        }

        #[test]
        fn to_str() {
            KEYPAIRS.iter().for_each(|(expected_private_key, _, _)| {
                let private_key = ZcashPrivateKey::from(&expected_private_key, &Format::Sapling(None), &Network::Testnet).unwrap();
                test_to_str(expected_private_key, &private_key);
            });
        }
    }

    #[test]
    fn test_p2pkh_invalid() {

        // Invalid spending key length

        let private_key = "L";
        assert!(ZcashPrivateKey::from_str(private_key).is_err());

        let private_key = "L5hax5dZaByC3kJ4aLrZgnMXGSQReq";
        assert!(ZcashPrivateKey::from_str(private_key).is_err());

        let private_key = "L5hax5dZaByC3kJ4aLrZgnMXGSQReqRDYNqM1VAeXpqDRkRjX42";
        assert!(ZcashPrivateKey::from_str(private_key).is_err());

        let private_key = "L5hax5dZaByC3kJ4aLrZgnMXGSQReqRDYNqM1VAeXpqDRkRjX42HL5hax5dZaByC3kJ4aLrZgnMXGSQ";
        assert!(ZcashPrivateKey::from_str(private_key).is_err());

        let private_key = "L5hax5dZaByC3kJ4aLrZgnMXGSQReqRDYNqM1VAeXpqDRkRjX42HL5hax5dZaByC3kJ4aLrZgnMXGSQReqRDYNqM1VAeXpqDRkRjX42H";
        assert!(ZcashPrivateKey::from_str(private_key).is_err());

    }


    #[test]
    fn test_sapling_invalid() {

        // Invalid spending key length

        let private_key = "b";
        assert!(ZcashPrivateKey::sapling(private_key, &Network::Mainnet).is_err());

        let private_key = "bb69cdb5e70e2bbd24f771cd15a18ad58d3";
        assert!(ZcashPrivateKey::sapling(private_key, &Network::Mainnet).is_err());

        let private_key = "bb69cdb5e70e2bbd24f771cd15a18ad58d3ab9e1aa3cab186b9b65d17f7aade";
        assert!(ZcashPrivateKey::sapling(private_key, &Network::Mainnet).is_err());

        let private_key = "bb69cdb5e70e2bbd24f771cd15a18ad58d3ab9e1aa3cab186b9b65d17f7aadefbb69cdb5e70e2bbd24f771cd15a18ad58";
        assert!(ZcashPrivateKey::sapling(private_key, &Network::Mainnet).is_err());

        let private_key = "bb69cdb5e70e2bbd24f771cd15a18ad58d3ab9e1aa3cab186b9b65d17f7aadefbb69cdb5e70e2bbd24f771cd15a18ad58d3ab9e1aa3cab186b9b65d17f7aadef";
        assert!(ZcashPrivateKey::sapling(private_key, &Network::Mainnet).is_err());

    }
}