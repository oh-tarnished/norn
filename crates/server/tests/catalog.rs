//! EntityService, SignalService and StreamService over real gRPC.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use common::{start, temp};
use norn_protocol::entity::entity_service_client::EntityServiceClient;
use norn_protocol::entity::*;
use norn_protocol::rpc::StatusExt;
use norn_server::DaemonConfig;
use prost_types::FieldMask;
use tonic::Code;

fn reason(status: &tonic::Status) -> String {
    status
        .get_details_error_info()
        .expect("exactly one ErrorInfo")
        .reason
}

fn create(id: &str, name: &str, site: &str) -> CreateEntityRequest {
    CreateEntityRequest {
        entity_id: id.into(),
        entity: Some(Entity {
            display_name: name.into(),
            annotations: [("site".to_owned(), site.to_owned())].into(),
            ..Entity::default()
        }),
        validate_only: false,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn entities_are_created_read_updated_and_deleted_and_survive_a_restart() {
    let d = start(DaemonConfig::default(), temp("catalog")).await;
    let mut c = EntityServiceClient::new(d.channel.clone());
    let made = c
        .create_entity(create("a", "A", "north"))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(made.name, "entities/a");
    let again = c
        .create_entity(create("a", "A", "north"))
        .await
        .unwrap_err();
    assert_eq!(again.code(), Code::AlreadyExists);
    assert_eq!(reason(&again), "ERROR_REASON_RESOURCE_ALREADY_EXISTS");

    let mut change = made.clone();
    change.display_name = "Renamed".into();
    change.annotations.clear();
    let request = UpdateEntityRequest {
        entity: Some(change.clone()),
        update_mask: Some(FieldMask {
            paths: vec!["display_name".into()],
        }),
        validate_only: false,
    };
    let updated = c.update_entity(request.clone()).await.unwrap().into_inner();
    assert_eq!(updated.display_name, "Renamed");
    assert_eq!(updated.annotations["site"], "north", "the mask spared it");
    let stale = c.update_entity(request).await.unwrap_err();
    assert_eq!(stale.code(), Code::Aborted);
    assert_eq!(reason(&stale), "ERROR_REASON_ETAG_MISMATCH");

    let data = d.stop().await;
    let d = start(DaemonConfig::default(), data).await;
    let mut c = EntityServiceClient::new(d.channel.clone());
    let get = GetEntityRequest {
        name: "entities/a".into(),
    };
    assert_eq!(
        c.get_entity(get.clone()).await.unwrap().into_inner(),
        updated
    );
    let delete = DeleteEntityRequest {
        name: "entities/a".into(),
        ..Default::default()
    };
    c.delete_entity(delete).await.unwrap();
    let gone = c.get_entity(get).await.unwrap_err();
    assert_eq!(gone.code(), Code::NotFound);
    assert_eq!(reason(&gone), "ERROR_REASON_RESOURCE_NOT_FOUND");
    d.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn listing_pages_through_everything_and_a_token_is_bound_to_its_request() {
    let d = start(DaemonConfig::default(), temp("catalog")).await;
    let mut c = EntityServiceClient::new(d.channel.clone());
    for i in 0..5 {
        let site = if i % 2 == 0 { "north" } else { "south" };
        c.create_entity(create(&format!("e{i}"), "", site))
            .await
            .unwrap();
    }
    let mut names = Vec::new();
    let mut page_token = String::new();
    loop {
        let list = ListEntitiesRequest {
            page_size: 2,
            page_token,
            filter: String::new(),
        };
        let page = c.list_entities(list).await.unwrap().into_inner();
        names.extend(page.entities.into_iter().map(|e| e.name));
        page_token = page.next_page_token;
        if page_token.is_empty() {
            break;
        }
        let other = ListEntitiesRequest {
            page_size: 2,
            page_token: page_token.clone(),
            filter: "display_name = \"x\"".into(),
        };
        let refused = c.list_entities(other).await.unwrap_err();
        assert_eq!(reason(&refused), "ERROR_REASON_INVALID_TOKEN");
    }
    assert_eq!(
        names,
        [
            "entities/e0",
            "entities/e1",
            "entities/e2",
            "entities/e3",
            "entities/e4"
        ]
    );
    let filter = "annotations.site = \"north\"".to_owned();
    let list = ListEntitiesRequest {
        filter,
        ..Default::default()
    };
    assert_eq!(
        c.list_entities(list)
            .await
            .unwrap()
            .into_inner()
            .entities
            .len(),
        3
    );
    let list = ListEntitiesRequest {
        filter: "site ~ 1".into(),
        ..Default::default()
    };
    let bad = c.list_entities(list).await.unwrap_err();
    assert_eq!(bad.code(), Code::InvalidArgument);
    assert_eq!(reason(&bad), "ERROR_REASON_INVALID_ARGUMENT");
    assert!(
        bad.get_details_bad_request().is_some(),
        "and the field violation"
    );
    d.stop().await;
}
