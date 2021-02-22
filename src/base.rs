use bson::{doc, oid::ObjectId, Bson, Document};

use log::{debug, warn};
use mongodb::options::{FindOneOptions, FindOptions, InsertManyOptions, UpdateOptions};
use mongodb::Collection;
use mongodb_cursor_pagination::{CursorDirections, FindResult, PaginatedCursor};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::time::SystemTime;
use voca_rs::case::snake_case;

use crate::error::ServiceError;
use crate::id::ID;
use crate::node::Node;
use async_trait::async_trait;

#[derive(Serialize, Deserialize)]
pub struct DeleteResponse {
    id: ID,
    success: bool,
}

#[cfg(feature = "graphql")]
#[derive(Serialize, Deserialize, juniper::GraphQLObject)]
pub struct DeleteResponseGQL {
    id: ID,
    success: bool,
}

#[cfg(feature = "graphql")]
impl From<DeleteResponse> for DeleteResponseGQL {
    fn from(d: DeleteResponse) -> DeleteResponseGQL {
        DeleteResponseGQL {
            id: d.id.into(),
            success: d.success,
        }
    }
}

const DEFAULT_LIMIT: i64 = 25;

#[cfg(not(any(test, feature = "test")))]
fn now() -> SystemTime {
    SystemTime::now()
}

#[cfg(any(test, feature = "test"))]
pub mod mock_time {
    use super::*;
    use std::sync::{Arc, Mutex};
    use std::thread::spawn;
    use std::time::Duration;

    lazy_static! {
        static ref MOCK_TIME: Arc<Mutex<Option<SystemTime>>> = Arc::new(Mutex::new(None));
    }

    pub fn now() -> SystemTime {
        let mock_time = Arc::clone(&MOCK_TIME);
        let time_handle = spawn(move || {
            let time = mock_time.lock().unwrap().unwrap_or_else(SystemTime::now);
            let cloned = time.clone();
            cloned
        });
        time_handle.join().unwrap()
    }

    #[allow(dead_code)]
    pub fn increase_mock_time(millis: u64) {
        let mock_time = Arc::clone(&MOCK_TIME);
        let current_time = now();

        spawn(move || {
            let mut data_mutex = mock_time.lock().unwrap();
            *data_mutex = Some(current_time + Duration::from_millis(millis));
        });
    }

    #[allow(dead_code)]
    pub fn set_mock_time(time: SystemTime) {
        let mock_time = Arc::clone(&MOCK_TIME);
        spawn(move || {
            let mut data_mutex = mock_time.lock().unwrap();
            *data_mutex = Some(time);
        });
    }

    #[allow(dead_code)]
    pub fn clear_mock_time() {
        let mock_time = Arc::clone(&MOCK_TIME);
        spawn(move || {
            let mut data_mutex = mock_time.lock().unwrap();
            *data_mutex = None;
        });
    }
}

#[cfg(any(test, feature = "test"))]
pub use mock_time::now;

#[async_trait]
pub trait BaseService<'a> {
    fn new(collection: &Collection, default_sort: Option<Document>) -> Self;
    fn id_parameter(&self) -> &'static str {
        "_id"
    }
    fn data_source(&self) -> &Collection;
    fn default_sort(&self) -> Document {
        doc! { self.id_parameter(): 1 }
    }
    fn default_filter(&self) -> Option<&Document> {
        None
    }
    fn default_limit(&self) -> i64 {
        DEFAULT_LIMIT
    }
    fn generate_id(&self) -> Option<String> {
        None
    }

    async fn find<T>(
        &'a self,
        filter: Option<Document>,
        sort: Option<Document>,
        limit: Option<i32>,
        after: Option<String>,
        before: Option<String>,
        skip: Option<i32>,
    ) -> Result<FindResult<T>, ServiceError>
    where
        T: DeserializeOwned + Send,
    {
        let coll = self.data_source();
        // build the options object
        let find_options = FindOptions::builder()
            .limit(if let Some(l) = limit {
                l as i64
            } else {
                self.default_limit()
            })
            .skip(if let Some(s) = skip { s as i64 } else { 0 })
            // TODO: make this not something arbitrary for testing purposes
            .sort(if let Some(s) = sort {
                s
            } else {
                self.default_sort()
            })
            .build();
        let is_previous_query = before.is_some() && after.is_none();
        let query_cursor = if is_previous_query {
            PaginatedCursor::new(Some(find_options), before, Some(CursorDirections::Previous))
        } else {
            PaginatedCursor::new(Some(find_options), after, None)
        };
        let find_results: FindResult<T> = if let Some(f) = filter {
            match query_cursor.find(&coll, Some(&f)).await {
                Ok(q) => q,
                Err(e) => return Err(ServiceError::from(e)),
            }
        } else {
            match query_cursor.find(&coll, self.default_filter()).await {
                Ok(q) => q,
                Err(e) => return Err(ServiceError::from(e)),
            }
        };
        println!("!MONGO BASE SERVICE FIND QUERY");
        Ok(find_results)
    }

    // fn aggregate<T>(
    //     &self,
    //     pipeline: Option<Document>,
    //     sort: Option<Document>,
    //     limit: Option<i32>,
    //     after: Option<String>,
    //     before: Option<String>,
    //     skip: Option<i32>,
    // ) -> Result<AggregateResult<T>, ServiceError>
    // where
    //     T: serde::Deserialize<'a>,
    // {
    //     let coll = self.data_source();
    //     // build the options object
    //     let aggregate_options = AggregateOptions::builder()
    //         .limit(if let Some(l) = limit {
    //             l as i64
    //         } else {
    //             self.default_limit()
    //         })
    //         .skip(if let Some(s) = skip { s as i64 } else { 0 })
    //         // TODO: make this not something arbitrary for testing purposes
    //         .sort(if let Some(s) = sort {
    //             s
    //         } else {
    //             self.default_sort()
    //         })
    //         .build();
    //     let is_previous_query = before.is_some() && after.is_none();
    //     let query_cursor = if is_previous_query {
    //         PaginatedCursor::new(Some(aggregate_options), before, Some(CursorDirections::Previous))
    //     } else {
    //         PaginatedCursor::new(Some(aggregate_options), after, None)
    //     };
    //     let aggregate_results: AggregateResult<T> = if let Some(f) = pipeline {
    //         query_cursor.aggregate(&coll, Some(&f))?
    //     } else {
    //         query_cursor.aggregate(&coll, self.default_filter())?
    //     };
    //     Ok(aggregate_results)
    // }

    // fn get_embedded_by_id<U>(
    //     &self,
    //     id: ID,
    //     field: &str,
    //     limit: Option<i32>,
    //     skip: Option<i32>,
    // ) -> Result<Vec<U>, ServiceError>
    // where
    //     U: serde::Deserialize<'a>,
    // {
    //     let coll = self.data_source();
    //     let find_options = FindOneOptions::builder()
    //         .projection(Some(doc! {
    //             field: {
    //                 "$slice": [ skip.unwrap_or(0), limit.unwrap_or(self.default_limit() as i32) ]
    //             }
    //         }))
    //         .build();
    //     let query = Some(doc! { self.id_parameter(): id.to_bson() });
    //     let find_result = coll.find_one(query, Some(find_options))?;
    //     match find_result {
    //         Some(result) => {
    //             let embedded_result = result.get_array(field);
    //             match embedded_result {
    //                 Ok(embedded) => {
    //                     let docs = bson::from_bson(bson::Bson::Array(embedded.clone()))?;
    //                     Ok(docs)
    //                 }
    //                 Err(e) => Err(ServiceError::ParseError(e.to_string())),
    //             }
    //         }
    //         None => Ok(Vec::new()),
    //     }
    // }

    async fn search<T>(
        &'a self,
        search_term: String,
        fields: Vec<String>,
        sort: Option<Document>,
        limit: Option<i32>,
        after: Option<String>,
        before: Option<String>,
        skip: Option<i32>,
    ) -> Result<FindResult<T>, ServiceError>
    where
        T: DeserializeOwned + Send,
    {
        let coll = self.data_source();
        // build the options object
        let find_options = FindOptions::builder()
            .limit(if let Some(l) = limit {
                l as i64
            } else {
                self.default_limit()
            })
            .skip(if let Some(s) = skip { s as i64 } else { 0 })
            // TODO: make this not something arbitrary for testing purposes
            .sort(if let Some(s) = sort {
                s
            } else {
                self.default_sort()
            })
            .build();
        let is_previous_query = before.is_some() && after.is_none();
        let query_cursor = if is_previous_query {
            PaginatedCursor::new(Some(find_options), before, Some(CursorDirections::Previous))
        } else {
            PaginatedCursor::new(Some(find_options), after, None)
        };
        let mut filter = doc! { "$or": [] };
        let or_array = filter.get_array_mut("$or").unwrap();
        for field in fields.iter().map(|f| snake_case(&f)) {
            or_array.push(Bson::Document(
                doc! { 
                    field: Bson::RegularExpression(
                        bson::Regex { pattern: search_term.clone(),  options: "i".to_string() }
                    )
                }
            ));
        }
        let find_results: FindResult<T> = match query_cursor.find(&coll, Some(&filter)).await {
            Ok(r) => r,
            Err(e) => return Err(ServiceError::from(e)),
        };
        Ok(find_results)
    }

    async fn find_one<T>(&'a self, filter: Document) -> Result<Option<T>, ServiceError>
    where
        T: DeserializeOwned,
    {
        let coll = self.data_source();
        let find_result = coll.find_one(filter, None).await;
        match Some(find_result) {
            Some(item_doc) => {
                let doc = bson::from_bson(bson::Bson::Document(item_doc.unwrap().unwrap()))?;
                Ok(Some(doc))
            }
            None => Ok(None),
        }
    }

    async fn find_one_by_object_id<T>(
        &'a self,
        field: &str,
        value: ObjectId,
    ) -> Result<Option<T>, ServiceError>
    where
        T: DeserializeOwned + Send,
    {
        let coll = self.data_source();
        let query = Some(doc! { field: value });
        let find_result = match coll.find_one(query, None).await {
            Ok(f) => f,
            Err(e) => return Err(ServiceError::from(e))
        };
        match find_result {
            Some(item_doc) => {
                let doc = bson::from_bson(bson::Bson::Document(item_doc))?;
                Ok(Some(doc))
            }
            None => Ok(None),
        }
    }

    async fn find_one_by_id<T>(&'a self, id: ID) -> Result<Option<T>, ServiceError>
    where
        T: DeserializeOwned + Send,
    {
        match id {
            ID::String(s) => match self.find_one_by_string_value(self.id_parameter(), &s).await {
                Ok(r) => Ok(r),
                Err(e) => return Err(ServiceError::from(e))
            },
            ID::I64(i) => match self.find_one_by_i64(self.id_parameter(), i).await {
                Ok(r) => Ok(r),
                Err(e) => return Err(ServiceError::from(e))
            },
            ID::ObjectId(o) => match self.find_one_by_object_id(self.id_parameter(), o).await {
                Ok(r) => Ok(r),
                Err(e) => return Err(ServiceError::from(e))
            },
        }
    }

    async fn find_one_by_string_value<T>(
        &'a self,
        field: &str,
        value: &str,
    ) -> Result<Option<T>, ServiceError>
    where
        T: DeserializeOwned + Send
    {
        let coll = self.data_source();
        let query = Some(doc! { field: value });
        let find_result = match coll.find_one(query, None).await {
            Ok(r) => r,
            Err(e) => return Err(ServiceError::from(e))
        };
        match find_result {
            Some(item_doc) => {
                let doc = bson::from_bson(bson::Bson::Document(item_doc))?;
                Ok(Some(doc))
            }
            None => Ok(None),
        }
    }

    async fn find_one_by_i64<T>(&'a self, field: &str, value: i64) -> Result<Option<T>, ServiceError>
    where
        T: DeserializeOwned + Send,
    {
        let coll = self.data_source();
        let query = Some(doc! { field: value });
        let find_result = match coll.find_one(query, None).await {
            Ok(r) => r,
            Err(e) => return Err(ServiceError::from(e))
        };
        match find_result {
            Some(item_doc) => {
                let doc = bson::from_bson(bson::Bson::Document(item_doc))?;
                Ok(Some(doc))
            }
            None => Ok(None),
        }
    }

    async fn insert_embedded<T>(
        &'a self,
        id: ID,
        field_path: &str,
        new_items: Vec<T>,
        user_id: Option<ID>,
    ) -> Result<Vec<ID>, ServiceError>
    where
        T: serde::Serialize + Send,
    {
        // get the item
        let coll = self.data_source();
        let query = doc! { self.id_parameter(): id.to_bson() };
        let find_result = match coll.find_one(Some(query.clone()), None).await {
            Ok(r) => r,
            Err(e) => return Err(ServiceError::from(e))
        };
        let mut inserted_ids: Vec<ID> = Vec::new();
        let timestamp = now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("Unable to retrieve time")
            .as_secs();
        match find_result {
            None => Err(ServiceError::NotFound("Unable to find item".into())),
            Some(_item) => {
                // insert it
                let serialized_members = new_items.iter().fold(Vec::new(), |mut acc, item| {
                    match bson::to_bson(&item) {
                        Ok(serialized_member) => {
                            if let bson::Bson::Document(mut document) = serialized_member {
                                let mut node_details = Document::new();
                                node_details.insert("date_created", timestamp);
                                node_details.insert("date_modified", timestamp);
                                if let Some(uid) = &user_id {
                                    node_details.insert("created_by_id", uid.to_bson());
                                    node_details.insert("updated_by_id", uid.to_bson());
                                }
                                let fallback_id = uuid::Uuid::new_v4().to_hyphenated().to_string();
                                if let Some(insert_id) = document.get("_id") {
                                    match insert_id {
                                        Bson::Null => {
                                            document.insert("_id", &fallback_id);
                                            inserted_ids.push(ID::String(fallback_id));
                                        }
                                        _ => {
                                            let i: ID = ID::with_bson(insert_id);
                                            inserted_ids.push(i);
                                        }
                                    }
                                } else {
                                    document.insert("_id", &fallback_id);
                                    inserted_ids.push(ID::String(fallback_id));
                                }
                                document.insert("node", node_details);
                                acc.push(document);
                            }
                        }
                        Err(_) => warn!("Unable to insert item"),
                    }
                    acc
                });

                let update_doc = doc! { "$push": { field_path: { "$each": serialized_members } } };
                let _result = match coll.update_one(query, update_doc, None).await {
                    Ok(r) => r,
                    Err(e) => return Err(ServiceError::from(e))
                };
                Ok(inserted_ids)
            }
        }
    }

    async fn upsert_embedded<T, U>(
        &'a self,
        id: ID,
        field_path: &str,
        new_items: Vec<T>,
        user_id: Option<ID>,
        parent: Option<U>,
    ) -> Result<Vec<ID>, ServiceError>
    where
        T: serde::Serialize + Send,
        U: serde::Serialize + Send,
    {
        // get the item
        let coll = self.data_source();
        let query = doc! { self.id_parameter(): id.to_bson() };
        let mut inserted_ids: Vec<ID> = Vec::new();
        let timestamp = now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("Unable to retrieve time")
            .as_secs();
        // insert it
        let serialized_members = new_items.iter().fold(Vec::new(), |mut acc, item| {
            match bson::to_bson(&item) {
                Ok(serialized_member) => {
                    if let bson::Bson::Document(mut document) = serialized_member {
                        let mut node_details = Document::new();
                        node_details.insert("date_created", timestamp);
                        node_details.insert("date_modified", timestamp);
                        if let Some(uid) = &user_id {
                            node_details.insert("created_by_id", uid.to_bson());
                            node_details.insert("updated_by_id", uid.to_bson());
                        }
                        if let Some(insert_id) = document.get("_id") {
                            inserted_ids.push(ID::with_bson(insert_id));
                        } else {
                            let insert_id = uuid::Uuid::new_v4().to_hyphenated().to_string();
                            document.insert("_id", &insert_id);
                            inserted_ids.push(ID::with_string(insert_id));
                        }
                        document.insert("node", node_details);
                        acc.push(document);
                    }
                }
                Err(_) => warn!("Unable to insert item"),
            }
            acc
        });

        let mut update_doc = doc! { "$push": { field_path: { "$each": serialized_members } } };
        if parent.is_some() {
            let serialized_parent = bson::to_bson(&parent)?;
            update_doc.insert("$setOnInsert", serialized_parent);
        }
        let _result = match coll.update_one(
            query,
            update_doc,
            UpdateOptions::builder()
                .array_filters(None)
                .bypass_document_validation(None)
                .collation(None)
                .hint(None)
                .upsert(Some(true))
                .write_concern(None)
                .build(),
        ).await {
            Ok(r) => r,
            Err(e) => return Err(ServiceError::from(e))
        };
        Ok(inserted_ids)
    }

    async fn insert_one<T>(&'a self, new_item: T, user_id: Option<ID>) -> Result<ID, ServiceError>
    where
        T: serde::Serialize + Send,
    {
        let coll = self.data_source();
        let serialized_member = bson::to_bson(&new_item)?;
        let timestamp = now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("Unable to retrieve time")
            .as_secs();

        if let bson::Bson::Document(mut document) = serialized_member {
            let mut node_details = Document::new();
            node_details.insert("date_created", timestamp);
            node_details.insert("date_modified", timestamp);
            if let Some(uid) = &user_id {
                node_details.insert("created_by_id", uid.to_bson());
                node_details.insert("updated_by_id", uid.to_bson());
            }
            document.insert("node", node_details);
            // remove empty _id values
            if let Some(temp_id) = document.get(self.id_parameter()) {
                match temp_id {
                    Bson::Null => {
                        document.remove(self.id_parameter());
                    }
                    _ => debug!("id has value {}", temp_id),
                }
            }
            let result = match coll.insert_one(document, None).await {
                Ok(r) => r,
                Err(e) => return Err(ServiceError::from(e))
            };
            let id = ID::with_bson(&result.inserted_id);
            Ok(id)
        } else {
            warn!("Error converting the BSON object into a MongoDB document");
            Err(ServiceError::ParseError(
                "Error converting the BSON object into a MongoDB document".into(),
            ))
        }
    }

    async fn insert_many<T>(
        &'a self,
        new_items: Vec<T>,
        user_id: Option<ID>,
    ) -> Result<Vec<ID>, ServiceError>
    where
        T: serde::Serialize + Send,
    {
        let coll = self.data_source();
        let timestamp = now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("Unable to retrieve time")
            .as_secs();
        let serialized_members = new_items.iter().fold(Vec::new(), |mut acc, item| {
            match bson::to_bson(&item) {
                Ok(serialized_member) => {
                    if let bson::Bson::Document(mut document) = serialized_member {
                        let mut node_details = Document::new();
                        node_details.insert("date_created", timestamp);
                        node_details.insert("date_modified", timestamp);
                        if let Some(uid) = &user_id {
                            node_details.insert("created_by_id", uid.to_bson());
                            node_details.insert("updated_by_id", uid.to_bson());
                        }
                        document.insert("node", node_details);
                        acc.push(document);
                    }
                }
                Err(_) => warn!("Unable to insert item"),
            }
            acc
        });

        let result = match coll.insert_many(
            serialized_members,
            InsertManyOptions::builder()
                .bypass_document_validation(None)
                .ordered(Some(false))
                .write_concern(None)
                .build(),
        ).await {
            Ok(r) => r,
            Err(e) => return Err(ServiceError::from(e))
        };
        let ids: Vec<ID> = result
            .inserted_ids
            .values()
            .map(|i| ID::with_bson(i))
            .collect();

        Ok(ids)
    }

    async fn delete_one_by_id(&'a self, id: ID) -> Result<DeleteResponse, ServiceError> {
        let coll = self.data_source();
        let filter = doc! { self.id_parameter(): id.to_bson() };
        let result = coll.delete_one(filter, None).await;
        match result {
            Ok(r) => Ok(DeleteResponse {
                id,
                success: r.deleted_count == 1,
            }),
            Err(e) => Err(e.into()),
        }
    }

    async fn delete_one_by_query(&'a self, filter: Document) -> Result<bool, ServiceError> {
        let coll = self.data_source();
        let result = coll.delete_one(filter, None).await;
        match result {
            Ok(r) => Ok(r.deleted_count == 1),
            Err(e) => Err(e.into()),
        }
    }

    async fn delete_embedded(
        &'a self,
        id: ID,
        field_path: &str,
        embedded_id: ID,
    ) -> Result<DeleteResponse, ServiceError> {
        let coll = self.data_source();
        let query = doc! { self.id_parameter(): &id.to_bson() };
        let update_doc =
            doc! { "$pull": { field_path: { self.id_parameter(): &embedded_id.to_bson()} } };
        let _result = match coll.update_one(query, update_doc, None).await {
            Ok(r) => r,
            Err(e) => return Err(ServiceError::from(e))
        };
        Ok(DeleteResponse {
            id: embedded_id,
            success: true,
        })
    }

    async fn update_embedded<T, U>(
        &'a self,
        id: ID,
        field_path: &str,
        embedded_id: ID,
        update_item: T,
        user_id: Option<ID>,
    ) -> Result<U, ServiceError>
    where
        T: serde::Serialize + Send,
        U: DeserializeOwned,
    {
        let coll = self.data_source();
        let search_embedded = doc! {
            self.id_parameter(): &id.to_bson(),
            format!("{}.{}", field_path, self.id_parameter()): &embedded_id.to_bson(),
        };
        let serialized_member = bson::to_bson(&update_item)?;
        let timestamp = now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("Unable to retrieve time")
            .as_secs();
        if let bson::Bson::Document(document) = serialized_member {
            let array_path = format!("{}.$", field_path);
            let mut update_doc = Document::new();
            for key in document.keys() {
                let value = document.get(key);
                if let Some(v) = value {
                    update_doc.insert(format!("{}.{}", array_path, key), v.clone());
                }
            }
            update_doc.insert(format!("{}.node.date_modified", array_path), timestamp);
            if let Some(uid) = user_id {
                update_doc.insert(format!("{}.node.updated_by_id", array_path), uid.to_bson());
            }

            let update = doc! { "$set": update_doc };
            let search = doc! { self.id_parameter(): &id.to_bson() };
            match coll.update_one(search_embedded, update, None).await {
                Ok(_res) => match coll.find_one(Some(search), None).await {
                    Ok(res) => match res {
                        Some(doc) => {
                            let item: U = bson::from_bson(bson::Bson::Document(doc))?;
                            Ok(item)
                        }
                        None => Err(ServiceError::NotFound("Unable to find item".to_owned())),
                    },
                    Err(t) => {
                        warn!("Search failed");
                        Err(ServiceError::from(t))
                    }
                },
                Err(e) => Err(ServiceError::from(e)),
            }
        } else {
            Err("Unable to update document".into())
        }
    }

    async fn update_one<T, U>(
        &'a self,
        id: ID,
        update_item: T,
        user_id: Option<ID>,
    ) -> Result<U, ServiceError>
    where
        T: serde::Serialize + Send,
        U: DeserializeOwned,
    {
        let coll = self.data_source();
        let search = doc! { self.id_parameter(): id.to_bson() };
        let serialized_member = bson::to_bson(&update_item)?;
        let timestamp = now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("Unable to retrieve time")
            .as_secs();
        if let bson::Bson::Document(mut document) = serialized_member {
            document.insert("node.date_modified", timestamp);
            if let Some(uid) = user_id {
                document.insert("node.updated_by_id", uid.to_bson());
            }
            match coll.update_one(search.clone(), doc! {"$set": document}, None).await {
                Ok(_res) => match coll.find_one(Some(search), None).await {
                    Ok(res) => match res {
                        Some(doc) => {
                            let item: U = bson::from_bson(bson::Bson::Document(doc))?;
                            Ok(item)
                        }
                        None => Err(ServiceError::NotFound("Unable to find item".to_owned())),
                    },
                    Err(t) => {
                        warn!("Search failed");
                        Err(ServiceError::from(t))
                    }
                },
                Err(e) => Err(ServiceError::from(e)),
            }
        } else {
            Err("Invalid update document".into())
        }
    }

    async fn update_one_with_doc<T>(&'a self, id: ID, update_doc: Document) -> Result<T, ServiceError>
    where
        T: DeserializeOwned,
    {
        let coll = self.data_source();
        let search = doc! { self.id_parameter(): id.to_bson() };
        match coll.update_one(search.clone(), update_doc, None).await {
            Ok(_res) => match coll.find_one(Some(search), None).await {
                Ok(res) => match res {
                    Some(doc) => {
                        let item: T = bson::from_bson(bson::Bson::Document(doc))?;
                        Ok(item)
                    }
                    None => Err(ServiceError::NotFound("Unable to find item".to_owned())),
                },
                Err(t) => {
                    warn!("Search failed");
                    Err(ServiceError::from(t))
                }
            },
            Err(e) => Err(ServiceError::from(e)),
        }
    }
}
