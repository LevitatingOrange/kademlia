use crate::util::*;
use actix::dev::ToEnvelope;
use actix::prelude::*;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::fmt::Debug;

#[derive(Debug, MessageResponse)]
pub enum StorageResult<S: StorageActor> {
    NotFound,
    Found(S::StorageData),
}

pub trait StorageActor:
    'static
    + Actor<Context: ToEnvelope<Self, StorageMessage<Self>>>
    + Handler<StorageMessage<Self>>
    + Debug
{
    type StorageData: Serialize + DeserializeOwned + Send + Debug + Clone;
}

#[derive(Debug)]
pub enum StorageMessage<S: StorageActor> {
    Store(KademliaKey, S::StorageData),
    Retrieve(KademliaKey),
}
impl<S: StorageActor> Message for StorageMessage<S> {
    type Result = StorageResult<S>;
}
