use generic_array::{arr, ArrayLength, GenericArray};
use std::intrinsics::ctlz;

use crate::bucket::Bucket;
use actix::{Addr, Message};
use rand::{thread_rng, Rng};
//use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use std::fmt::{Display, Formatter};
use std::hash::Hash;
use std::net::SocketAddr;
use tiny_keccak::Keccak;

#[derive(PartialEq, Eq, Message, Debug, Clone, Serialize, Deserialize, Copy, Hash)]
pub struct Key(u128);

impl Key {
    pub fn distance(&self, other: &Key) -> usize {
        floor_nearest_power_of_two(self.0 ^ other.0)
    }
}

pub trait BucketListSize<K: FindNodeCount> =
    'static + ArrayLength<Addr<Bucket<Self, K>>> + PartialEq + Eq + Clone;
pub trait FindNodeCount = 'static + ArrayLength<Key> + PartialEq + Eq;

#[derive(PartialEq, Eq, Message, Debug, Clone, Serialize, Deserialize, Copy)]
#[serde(
    // tag = "cmd",
    // content = "data",
)]
pub struct Connection {
    pub address: SocketAddr,
    pub id: Key,
}

impl Connection {
    pub fn new(address: SocketAddr, id: Key) -> Self {
        Connection { address, id }
    }
}

impl Display for Connection {
    fn fmt(&self, f: &mut Formatter) -> Result<(), std::fmt::Error> {
        write!(f, "Peer {} with id {:?}", self.address, self.id)
    }
}

// pub fn xor_metric<N: KeyLength>(key1: &Key<N>, key2: &Key<N>) -> u128 {
//     let mut result = 0;
//     for (i, (k1, k2)) in key1.iter().zip(key2).enumerate() {
//         result = result | (((k1 ^ k2) as u128) << (i * 8));
//     }
//     result
// }

/// use intrinsic `ctlz` (count leading zeros) to calculate the nearest whole (floor) log2 of `val`
pub fn floor_nearest_power_of_two(val: u128) -> usize {
    if val == 0 {
        0
    } else {
        128 - (ctlz(val) as usize) - 1
    }
}

// pub fn create_key<N: Key, D: Into<Vec<u8>>>(data: Vec<D>) -> GenericArray<u8, N> {
//     // TODO: shake128 or shake256?
//     let mut hasher = Keccak::new_shake128();
//     for d in data {
//         hasher.update(&(d.into()));
//     }
//     let mut key = GenericArray::default();
//     hasher.finalize(&mut key);
//     key
// }

// pub fn create_node_id<N: Key>() -> GenericArray<u8, N> {
//     // TODO: shake128 or shake256?
//     let mut key: GenericArray<u8, N> = GenericArray::default();
//     let mut rng = thread_rng();
//     for k in key.as_mut_slice() {
//         // TODO which distribution does rng.gen use? conflicting results
//         *k = rng.gen();
//     }
//     key
// }

#[cfg(test)]
mod tests {
    use super::*;
    use generic_array::typenum::{U32, U64};
    use hex::decode;

    //#[test]
    // fn create_key_256() {
    //     let key1: GenericArray<u8, U32> = create_key(vec!["192.168.178.192"]);
    //     assert_eq!(
    //         key1.as_slice(),
    //         decode("d3378e05ca4f003afe70404148ba4741cd8f77e0c1d5a4800af19c801d5328aa")
    //             .unwrap()
    //             .as_slice()
    //     );
    //     let key2: GenericArray<u8, U32> = create_key(vec!["foo", "bar", "baz"]);
    //     assert_eq!(
    //         key2.as_slice(),
    //         decode("02cee818616be2012ed040818eb62b13d5fa77e6a778c4effcd0f12c5ffd1fba")
    //             .unwrap()
    //             .as_slice()
    //     );
    // }
    // #[test]
    // fn create_node_id_256() {
    //     let zero: GenericArray<u8, U32> = GenericArray::default();
    //     let key1: GenericArray<u8, U32> = create_node_id();
    //     assert!(key1.as_slice() != zero.as_slice());
    //     let key2: GenericArray<u8, U32> = create_node_id();
    //     assert!(key1.as_slice() != key2.as_slice());
    // }

    #[test]
    fn nearest_log2_floor() {
        let vals: [u128; 102] = [
            0, 2, 878921617, 529133195, 159006194, 627538282, 694854706, 550968384, 541767539,
            663743934, 19007286, 390725023, 124126637, 316649219, 586109768, 736891068, 461788159,
            886465885, 187810548, 782675677, 805918642, 148193673, 158704798, 145391545, 232797366,
            595940013, 961780566, 106333197, 162803638, 279777482, 110742971, 706669625, 553243471,
            595363152, 709926383, 432482078, 598424002, 41424562, 417785482, 898870363, 517938736,
            406367228, 4156519, 527849464, 122534161, 657901010, 431584246, 47131316, 438581871,
            919497058, 27831033, 807206312, 499522039, 97615362, 764737274, 115426289, 762937804,
            906882893, 969185408, 748116345, 795034984, 989001027, 965073501, 844945753, 503273917,
            325718503, 941467880, 41575162, 2703604, 343109977, 881940454, 718731040, 323941136,
            27203309, 565290492, 380948182, 37078651, 201734148, 880581459, 913668453, 988753634,
            928441507, 94940116, 997243591, 280047514, 460607758, 725143594, 711210996, 457999772,
            276723606, 444504114, 313935272, 424918384, 754113573, 650165373, 730537611, 319943631,
            154687402, 40628330, 960375553, 626778841, 714725419,
        ];
        for (i, val) in vals.iter().enumerate() {
            let expected = if i == 0 {
                0
            } else {
                (*val as f64).log2().floor() as usize
            };
            let actual = floor_nearest_power_of_two(*val);
            assert_eq!(expected, actual, "failed at {}", i);
        }
    }
    // #[test]
    // fn create_key_064() {
    //     // let key1: GenericArray<u8, U64> = create_key(vec!["192.168.178.192"]);
    //     // assert_eq!(key1.as_slice(), decode("9f3a90200b7332050786ccba02316d7d1f3c198db99d7ccbe56328e2ff3e956d2a09a1199cf3aba4f973fad93910dd8a5b479c05ba4bb526b57f1692a7e58da6").unwrap().as_slice());
    //     let key2: GenericArray<u8, U64> = create_key(vec!["foo","bar","baz"]);
    //     assert_eq!(key2.as_slice(), decode("7ed3ea82b7b49cdf484e178ea4f33647e6ca3b3a268cddd3f0781e1537af0f6862a0463180ee07495fbe16fa3c75cd0dba25598878639f406ea85e4cd01af5c5").unwrap().as_slice());
    // }
}
