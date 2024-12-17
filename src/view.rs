use crate::config::AppConfig;
use crate::config::Args;
use async_std::fs;
use async_std::task;
use couch_rs::database::Database;
use couch_rs::error::CouchResult;
use couch_rs::types::query::{QueriesParams, QueryParams};
use couch_rs::Client;
extern crate json;
use couch_rs::document::{DocumentCollection, TypedCouchDocument};
use couch_rs::types::document::DocumentId;
use couch_rs::types::find::FindQuery;
use couch_rs::types::system::DbInfo;
use couch_rs::CouchDocument;
use eframe::wgpu::hal::auxil::db;
use homedir::my_home;
use serde::{Deserialize, Serialize};
use serde_json::json;
use serde_json::{from_value, to_value, Value};
use std::borrow::Borrow;
use std::borrow::BorrowMut;
use std::collections::HashMap;
use std::error::Error;
use unescape::unescape;
#[derive(Serialize, Deserialize, CouchDocument)]
pub struct DocId {
    #[serde(skip_serializing_if = "String::is_empty")]
    pub _id: DocumentId,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub _rev: String,
}

pub async fn get_ids(
    db: Database,
    total: u64,
) -> Result<(HashMap<String, String>), Box<dyn Error>> {
    let limit = 1000;
    let mut h: HashMap<String, String> = HashMap::new();
    //h.insert("bookmark".to_string(), "".to_string());
    let mut v: Vec<String> = vec!["_id".to_string(), "_rev".to_string()];
    let mut find_all = FindQuery::find_all().limit(limit).fields(v.clone());
    let docs = db.find_raw(&find_all).await?;
    for i in docs.rows {
        let _id = i["_id"].as_str().unwrap();
        let _rev = i["_rev"].as_str().unwrap();
        h.insert(_id.to_string(), _rev.to_string());
    }

    let mut bookmark = docs.bookmark.unwrap().clone();
    let mut total_rows = docs.total_rows.clone();
    let mut sum = 0;
    //println!("{:?}", &total_rows);

    while total_rows > 0 {
        sum = sum + total_rows;

        //println!("...bookmark: {}", &bookmark);
        let mut find_all = FindQuery::find_all()
            .limit(limit)
            .fields(v.clone())
            .bookmark(&bookmark);
        let docs2 = db.find_raw(&find_all).await?;
        bookmark = docs2.clone().bookmark.unwrap().clone();
        total_rows = docs2.clone().total_rows;
        println!("{0}/{1} - {2}", sum, total, total_rows);

        for i in docs2.rows {
            let _id = i["_id"].as_str().unwrap();
            let _rev = i["_rev"].as_str().unwrap();
            h.insert(_id.to_string(), _rev.to_string());
        }
    }
    return Ok(h);
}

pub async fn delete_orphans(config: &AppConfig, args: Args) -> Result<(), Box<dyn Error>> {
    println!("...save_all_server_design fn");

    println!("xxxxxxxxxxxxxxxxxxxxxxxxxxxx");
    println!("master: {}", args.master);
    println!("repl: {}", args.repl);
    println!("db: {}", args.database);
    println!("user: {}", &config.user);
    println!("pass:{}", &config.password);
    println!("xxxxxxxxxxxxxxxxxxxxxxxxxxxx");

    let client = Client::new(&args.master, &config.user, &config.password)?;
    let number = client.get_info(&args.database).await?.doc_count;
    let db = client.db(&args.database).await?;
    let master_docs = get_ids(db, number).await?;

    let client2 = Client::new(&args.repl, &config.user, &config.password)?;
    let db2 = client2.db(&args.database).await?;
    let number = client2.get_info(&args.database).await?.doc_count;
    let repl_docs = get_ids(db2.clone(), number).await?;

    //  println!("repl docs: {}", &repl_docs.unwrap().keys().count());
    //    println!("master docs: {}", &master_docs.unwrap().keys().count());

    for (k, v) in &repl_docs {
        if !master_docs.contains_key(k) {
            //let _d: Value = db2.get(k).await?;
            //println!("{:?}", _d);
            let _id = unescape(&k).unwrap();
            let _rev = unescape(&v).unwrap();

            if let Some(doc) = db2.get::<Value>(&_id).await.ok() {
                db2.remove(&doc).await;
            }

            println!("Delete k: {} v: {} ", _id, _rev);
            /*
            let mut doc = json!({});
            doc.set_id(&_id);
            doc.set_rev(&_rev);
            println!("{:?}", doc);
            let b = db2.remove(&doc).await;
            */
        }
    }

    Ok(())
}

pub async fn save_all_server_design(config: &AppConfig) -> Result<(), Box<dyn Error>> {
    print!("...save_all_server_design fn");
    let client = Client::new(&config.host, &config.user, &config.password).unwrap();
    let dbs = client.list_dbs().await?;
    //    dbs.iter().for_each(|db| println!("Database: {}", db));
    for i in dbs {
        if !i.to_string().starts_with("_") {
            let mut config2 = config.clone();
            config2.database = i;
            //let new_config = config.borrow().clone();
            println!("...Database: {}", config2.database);

            save_all_design(&config2).await;
        }
    }
    Ok(())
}

pub async fn save_all_design(config: &AppConfig) -> Result<(), Box<dyn Error>> {
    let home = my_home().unwrap().unwrap();
    let client = Client::new(&config.host, &config.user, &config.password).unwrap();
    let db = client.db(&config.database).await;

    if db.is_ok() {
        let mut o = QueryParams::default();
        o.start_key = Some("_design".to_string());
        o.end_key = Some("_design0".to_string());
        //        o.limit = Some(3);
        o.include_docs = Some(true);

        let mut collections = db
            .unwrap()
            .query_many_all_docs(QueriesParams::new(vec![o]))
            .await?;

        //let mut c = _c.iter_mut();
        let mut collections = collections.iter_mut();
        let a = collections.next().unwrap();

        for i in a.rows.clone() {
            let mut doc = i.doc.unwrap();
            let mut j = json::parse(&doc.to_string()).unwrap();
            j.remove("_rev");

            let filename = format!(
                "{0}/Documents/{1}--{2}.json",
                home.display(),
                &config.database,
                j["_id"].to_string().replace("/", "__")
            );
            let data = j.dump();
            println!("...save {0}", filename);
            fs::write(filename, data).await;
        }
    }
    //return codes;

    Ok(())
}
