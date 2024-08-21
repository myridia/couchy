use crate::config::AppConfig;
use async_std::fs;
use async_std::task;
use couch_rs::error::CouchResult;
use couch_rs::types::query::{QueriesParams, QueryParams};
use couch_rs::Client;
extern crate json;
use homedir::my_home;
use std::borrow::Borrow;
use std::borrow::BorrowMut;
use std::collections::HashMap;
use std::error::Error;

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
