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

pub async fn get_ids(db: Database) -> Result<(HashMap<String, String>), Box<dyn Error>> {
    let mut h: HashMap<String, String> = HashMap::new();
    //h.insert("bookmark".to_string(), "".to_string());
    let mut v: Vec<String> = vec!["_id".to_string(), "_rev".to_string()];
    let mut find_all = FindQuery::find_all().limit(10000).fields(v.clone());
    let docs = db.find_raw(&find_all).await?;
    let mut bookmark = docs.bookmark.unwrap().clone();
    let mut total_rows = docs.total_rows;
    //println!("{:?}", bookmark);
    while total_rows > 0 {
        //println!("...bookmark: {}", &bookmark);
        let mut find_all = FindQuery::find_all()
            .limit(10000)
            .fields(v.clone())
            .bookmark(&bookmark);
        let docs2 = db.find_raw(&find_all).await?;
        //println!("{:?}", docs2.clone().total_rows);
        bookmark = docs2.clone().bookmark.unwrap().clone();
        total_rows = docs2.clone().total_rows;
        for i in docs2.rows {
            //println!("{}", i["_id"]);
            //println!("{}", i["_rev"]);
            h.insert(i["_id"].to_string(), i["_rev"].to_string());
        }
        //bookmark = "none".to_string();
    }
    return Ok(h);
}

pub async fn delete_orphans(config: &AppConfig, args: Args) -> Result<(), Box<dyn Error>> {
    println!("...save_all_server_design fn");

    println!("xxxxxxxxxxxxxxxxxxxxxxxxxxxx");
    println!("{:?}", args.master);
    println!("{:?}", args.repl);
    println!("{:?}", args.database);
    println!("{:?}", &config.user);
    println!("{:?}", &config.password);
    println!("xxxxxxxxxxxxxxxxxxxxxxxxxxxx");

    //let master = Client::new(&args.master, &config.user, &config.password)?;
    //let master_db = master.db(&args.database).await?;
    //let master_docs: DocumentCollection<DocId> = master_db.find(&find_all).await?;

    /*
    let client = Client::new(&args.repl, &config.user, &config.password)?;
    let db = client.db(&args.database).await?;
    let repl_docs = get_ids(db).await;
    */
    let client = Client::new(&args.master, &config.user, &config.password)?;
    let db = client.db(&args.database).await?;
    let master_docs = get_ids(db).await;

    //println!("{:?}", repl_docs.unwrap().keys().count());
    println!("{:?}", master_docs.unwrap().keys().count());
    /*

    let info = client.get_info(&args.database).await?;
    let number = info.doc_count;
    let mut repl: HashMap<String, String> = HashMap::new();

    let mut v: Vec<String> = vec!["_id".to_string(), "_rev".to_string()];
    let mut find_all = FindQuery::find_all().skip(0).limit(2).fields(v.clone());
    let docs = db.find_raw(&find_all).await?;
    println!("{:?}", docs);
    let mut find_all2 = FindQuery::find_all()
        .skip(0)
        .limit(2)
        .fields(v)
        .bookmark(&docs.bookmark.unwrap());
    let docs2 = db.find_raw(&find_all2).await?;
    println!("{:?}", docs2);
    */
    //println!("{:?}", &docs.bookmark);

    /*
    for i in 0..number {
        if i % 10000 == 0 || i == 0 {
            println!("{0}/{1}", i, number - i);

            /*
            let docs: DocumentCollection<DocId> = db.find(&find_all).await?;
            for i in docs.rows {
                //println!("{:?}", &i._id);
                //println!("{:?}", &i._rev);
                repl.insert(i._id, i._rev);
            }
            */
            //break;
        }
    }
    println!("{:?}", repl);
    */
    //let db = client.db(&args.database).await?;

    /*
    let docs: DocumentCollection<DocId> = db.find(&find_all).await?;


    */
    //println!("{:?}", repl_db.get);

    //for i in repl_docs.rows {
    //    println!("{:?}", &i._id);
    //    //println!("{:?}", &i._rev);
    //}

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
