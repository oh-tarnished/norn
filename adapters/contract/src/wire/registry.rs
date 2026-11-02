//! Making what a file declares exist: creating is safe to repeat.

use std::collections::{HashMap, HashSet};

use norn_protocol::common::ValueType;
use norn_protocol::entity::entity_service_client::EntityServiceClient;
use norn_protocol::entity::{CreateEntityRequest, Entity};
use norn_protocol::signal::signal_service_client::SignalServiceClient;
use norn_protocol::signal::{CreateSignalRequest, GetSignalRequest, Signal};
use norn_protocol::stream::stream_service_client::StreamServiceClient;
use norn_protocol::stream::{CreateStreamRequest, Stream};
use tonic::Code;
use tonic::transport::Channel;

use crate::{AdapterError, Catalog, entity_resource};

/// Which resources an import has ensured, and how many it had to create. Holds a
/// line per signal, entity and stream; never overwrites an existing resource's
/// metadata, because the target's may be newer than the file's.
pub(crate) struct Registry {
    channel: Channel,
    streams: HashSet<String>,
    entities: HashSet<String>,
    signals: HashMap<String, String>,
    pub(crate) created: u64,
}

impl Registry {
    pub(crate) fn new(channel: Channel) -> Self {
        Self {
            channel,
            streams: HashSet::new(),
            entities: HashSet::new(),
            signals: HashMap::new(),
            created: 0,
        }
    }

    /// Ensures everything `catalog` declares, parents first.
    pub(crate) async fn apply(&mut self, catalog: &Catalog) -> Result<(), AdapterError> {
        for stream in &catalog.streams {
            self.ensure_stream(&stream.name, stream).await?;
        }
        for entity in &catalog.entities {
            self.ensure_entity(&entity.name, entity).await?;
        }
        for signal in &catalog.signals {
            self.ensure_signal(signal).await?;
        }
        Ok(())
    }

    /// The stream a signal this registry ensured records into.
    pub(crate) fn stream_of(&self, signal: &str) -> Option<&str> {
        self.signals.get(signal).map(String::as_str)
    }

    async fn ensure_stream(&mut self, name: &str, stream: &Stream) -> Result<(), AdapterError> {
        if self.streams.contains(name) {
            return Ok(());
        }
        let id = name.strip_prefix("streams/").unwrap_or(name).to_owned();
        let request = CreateStreamRequest {
            stream_id: id,
            stream: Some(Stream {
                display_name: stream.display_name.clone(),
                annotations: stream.annotations.clone(),
                ..Stream::default()
            }),
            validate_only: false,
        };
        let mut client = StreamServiceClient::new(self.channel.clone());
        let result = client.create_stream(request).await.map(|_| ());
        self.created += self.created_or_existed("StreamService.CreateStream", result)?;
        self.streams.insert(name.to_owned());
        Ok(())
    }

    async fn ensure_entity(&mut self, name: &str, entity: &Entity) -> Result<(), AdapterError> {
        if self.entities.contains(name) {
            return Ok(());
        }
        let id = name.strip_prefix("entities/").unwrap_or(name).to_owned();
        let request = CreateEntityRequest {
            entity_id: id,
            entity: Some(Entity {
                display_name: entity.display_name.clone(),
                annotations: entity.annotations.clone(),
                ..Entity::default()
            }),
            validate_only: false,
        };
        let mut client = EntityServiceClient::new(self.channel.clone());
        let result = client.create_entity(request).await.map(|_| ());
        self.created += self.created_or_existed("EntityService.CreateEntity", result)?;
        self.entities.insert(name.to_owned());
        Ok(())
    }

    /// Ensures a signal and its parents. An existing one must agree on what can
    /// never change: its value type and its stream.
    pub(crate) async fn ensure_signal(&mut self, signal: &Signal) -> Result<(), AdapterError> {
        if self.signals.contains_key(&signal.name) {
            return Ok(());
        }
        let Some((entity, id)) = entity_resource(&signal.name) else {
            return Err(AdapterError::Conflict {
                resource: signal.name.clone(),
                detail: "not entities/{entity}/signals/{signal}".to_owned(),
            });
        };
        self.ensure_stream(&signal.stream, &Stream::default())
            .await?;
        let parent = format!("entities/{entity}");
        self.ensure_entity(&parent, &Entity::default()).await?;
        let request = CreateSignalRequest {
            parent,
            signal_id: id.to_owned(),
            signal: Some(Signal {
                name: String::new(),
                ..signal.clone()
            }),
            validate_only: false,
        };
        let mut client = SignalServiceClient::new(self.channel.clone());
        let result = client.create_signal(request).await.map(|_| ());
        let made = self.created_or_existed("SignalService.CreateSignal", result)?;
        self.created += made;
        if made == 0 {
            self.check_existing(&mut client, signal).await?;
        }
        (self.signals).insert(signal.name.clone(), signal.stream.clone());
        Ok(())
    }

    async fn check_existing(
        &self,
        client: &mut SignalServiceClient<Channel>,
        signal: &Signal,
    ) -> Result<(), AdapterError> {
        let name = signal.name.clone();
        let found = client.get_signal(GetSignalRequest { name }).await;
        let found = found.map_err(AdapterError::call("SignalService.GetSignal"))?;
        let found = found.into_inner();
        let word = |n| ValueType::try_from(n).unwrap_or(ValueType::Unspecified);
        let detail = if found.value_type != signal.value_type {
            let (have, file) = (word(found.value_type), word(signal.value_type));
            format!(
                "the daemon's is {}, the file's {}",
                have.as_str_name(),
                file.as_str_name()
            )
        } else if found.stream != signal.stream {
            let (have, file) = (&found.stream, &signal.stream);
            format!("the daemon's records into {have}, the file's into {file}")
        } else {
            return Ok(());
        };
        Err(AdapterError::Conflict {
            resource: signal.name.clone(),
            detail,
        })
    }

    /// 1 if created, 0 if it already existed, else the error.
    fn created_or_existed(
        &self,
        method: &'static str,
        result: Result<(), tonic::Status>,
    ) -> Result<u64, AdapterError> {
        match result {
            Ok(()) => Ok(1),
            Err(status) if status.code() == Code::AlreadyExists => Ok(0),
            Err(status) => Err(AdapterError::Call { method, status }),
        }
    }
}
