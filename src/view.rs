use crate::config::AppConfig;
use crate::config::Args;
use async_std::fs;
use couch_rs::Client;
use couch_rs::database::Database;
use couch_rs::types::query::{QueriesParams, QueryParams};
extern crate json;
use couch_rs::document::DocumentCollection;
use couch_rs::document::TypedCouchDocument;
use couch_rs::types::find::FindQuery;
use futures::future::join_all;
use homedir::my_home;
use serde_json::Value;
use serde_json::json;
use std::thread;

use tokio::sync::{
    mpsc,
    mpsc::{Receiver, Sender},
};

use std::collections::HashMap;
use std::error::Error;

// https://docs.rs/couch_rs/latest/couch_rs/database/struct.Database.html#method.remove

pub async fn get_ids(db: Database, total: u64) -> Result<HashMap<String, String>, Box<dyn Error>> {
    let limit = 1000;
    let mut h: HashMap<String, String> = HashMap::new();
    let v: Vec<String> = vec!["_id".to_string(), "_rev".to_string()];
    let find_all = FindQuery::find_all().limit(limit).fields(v.clone());
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
        let find_all = FindQuery::find_all()
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
    println!("key: {}", args.key);
    println!("value:: {}", args.value);
    println!("db: {}", args.db);
    println!("user: {}", &config.user);
    println!("pass:{}", &config.password);
    println!("xxxxxxxxxxxxxxxxxxxxxxxxxxxx");

    let client = Client::new(&args.master, &config.user, &config.password)?;
    let number = client.get_info(&args.db).await?.doc_count;
    let db = client.db(&args.db).await?;
    let master_docs = get_ids(db, number).await?;

    let client2 = Client::new(&args.repl, &config.user, &config.password)?;
    let db2 = client2.db(&args.db).await?;
    let number = client2.get_info(&args.db).await?.doc_count;
    let repl_docs = get_ids(db2.clone(), number).await?;

    for (k, v) in &repl_docs {
        if !master_docs.contains_key(k) {
            //let _d: Value = db2.get(k).await?;
            //println!("{:?}", _d);
            println!("Delete k: {} v: {} ", k, v);
            let mut doc = json!({});
            doc.set_id(&k);
            doc.set_rev(&v);
            //println!("{:?}", doc);
            let b = db2.remove(&doc).await;
            println!("...delete: {:?}", b);
        }
    }

    Ok(())
}

pub async fn worker(db: Database, docs: Vec<Vec<String>>) -> Result<(), Box<dyn Error>> {
    let total = docs.len();
    //println!("worker received {}x delete requests", docs.len());
    let mut c = total;
    let mut v: Vec<Value> = Vec::new();
    for i in docs {
        let doc = json!({"_id":i[0],"_rev":i[1]});
        println!("{0}/{1} - {2}", total, c, i[0]);
        //v.push(doc.clone());
        db.remove(&doc).await;
        c -= 1;
    }
    //let r = db.bulk_upsert(&mut v).await;
    //println!("{:?}", r);
    Ok(())
}
pub async fn delete_by_key(config: &AppConfig, args: Args) -> Result<(), Box<dyn Error>> {
    println!("...delete by key fn");

    println!("xxxxxxxxxxxxxxxxxxxxxxxxxxxx");
    println!("key: {}", args.key);
    println!("value: {}", args.value);
    println!("db: {}", args.db);
    println!("host:{}", &config.host);
    println!("user: {}", &config.user);
    println!("pass:{}", &config.password);
    println!("xxxxxxxxxxxxxxxxxxxxxxxxxxxx");

    let client = Client::new(&config.host, &config.user, &config.password)?;

    let db = client.db(&args.db).await?;

    let (tx, mut rx): (
        Sender<DocumentCollection<Value>>,
        Receiver<DocumentCollection<Value>>,
    ) = mpsc::channel(1000);

    let selectors = json!({ args.key: args.value});
    println!("{:?}", selectors);

    let fields = vec!["_id".to_string(), "_rev".to_string()];
    //let find = FindQuery::new(selectors).fields(fields).limit(40000);
    let find = FindQuery::new(selectors).fields(fields).limit(1000);
    println!("{:?}", find);
    let r = db.find_batched(find, tx, 500, 1500000).await;
    println!("{:?}", r);

    let mut c = 0;
    let db2 = client.db(&args.db).await?;
    let mut chunks: Vec<Vec<Vec<String>>> = Vec::new();

    while let Some(all_docs) = rx.recv().await {
        println!("Received {} docs", all_docs.total_rows);
        let mut docs: Vec<Vec<String>> = Vec::new();
        for r in all_docs.rows {
            let _id = r["_id"].as_str().unwrap().to_string();
            let _rev = r["_rev"].as_str().unwrap().to_string();
            let v = vec![_id, _rev];
            docs.push(v);
        }
        chunks.push(docs);
    }

    let mut futures = vec![worker(db2.clone(), chunks[0].clone())];
    c = 0;
    for i in chunks {
        if c > 0 {
            let t = worker(db2.clone(), i);
            futures.push(t);
        }
        c += 1;
    }
    join_all(futures).await;
    println!("....finished");

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

            let _r = save_all_design(&config2).await;
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
            let doc = i.doc.unwrap();
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
            let _r = fs::write(filename, data).await;
        }
    }
    //return codes;

    Ok(())
}
