use crate::config::AppConfig;
use crate::config::Args;
use async_std::fs;
use async_std::task;
use couch_rs::error::CouchResult;
use couch_rs::types::query::{QueriesParams, QueryParams};
use couch_rs::Client;
extern crate json;
use couch_rs::document::{DocumentCollection, TypedCouchDocument};
use couch_rs::types::document::DocumentId;
use couch_rs::types::find::FindQuery;
use couch_rs::CouchDocument;
use homedir::my_home;
use serde::{Deserialize, Serialize};
use std::borrow::Borrow;
use std::borrow::BorrowMut;
use std::collections::HashMap;
use std::error::Error;

#[derive(Serialize, Deserialize, CouchDocument)]
pub struct DocId {
    #[serde(skip_serializing_if = "String::is_empty")]
    pub _id: DocumentId,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub _rev: String,
}

pub async fn delete_orphans(config: &AppConfig, args: Args) -> Result<(), Box<dyn Error>> {
    println!("...save_all_server_design fn");

    println!("xxxxxxxxxxxxxxxxxxxxxxxxxxxx");
    println!("{:?}", args.master);
    println!("{:?}", args.repl);
    println!("{:?}", args.database);
    println!("{:?}", &config.user);
    println!("{:?}", &config.password);
    print!("xxxxxxxxxxxxxxxxxxxxxxxxxxxx");

    let find_all = FindQuery::find_all().limit(0);

    //let master = Client::new(&args.master, &config.user, &config.password)?;
    //let master_db = master.db(&args.database).await?;
    //let master_docs: DocumentCollection<DocId> = master_db.find(&find_all).await?;

    let repl = Client::new(&args.repl, &config.user, &config.password)?;
    let repl_db = repl.db(&args.database).await?;
    let repl_docs: DocumentCollection<DocId> = repl_db.find(&find_all).await?;

    for i in repl_docs.rows {
        println!("{:?}", &i._id);
        //println!("{:?}", &i._rev);
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
