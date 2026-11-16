use crate::config::AppConfig;
use crate::config::Args;
use couch_rs::Client;
use couch_rs::database::Database;
use couch_rs::types::query::{QueriesParams, QueryParams};
extern crate json;
use couch_rs::document::DocumentCollection;
use couch_rs::document::TypedCouchDocument;
use couch_rs::types::find::FindQuery;
use futures::future::join_all;
use serde_json::Value;
use serde_json::json;

use tokio::sync::{
    mpsc,
    mpsc::{Receiver, Sender},
};

use std::collections::HashMap;
use std::error::Error;
use std::sync::{Arc, Mutex};

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

    let mut bookmark = match docs.bookmark {
        Some(b) => b.clone(),
        None => return Ok(h),
    };
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
        bookmark = match docs2.bookmark {
            Some(b) => b.clone(),
            None => break,
        };
        total_rows = docs2.total_rows;
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
    println!("...delete_orphans fn");

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
    let mut c = total;
    for i in docs {
        let doc = json!({"_id":i[0],"_rev":i[1]});
        println!("{0}/{1} - {2}", total, c, i[0]);
        let _ = db.remove(&doc).await;
        c -= 1;
    }
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
    ) = mpsc::channel(100);

    let selectors = json!({ args.key: args.value});
    println!("{:?}", selectors);

    let fields = vec!["_id".to_string(), "_rev".to_string()];
    //let find = FindQuery::new(selectors).fields(fields).limit(40000);
    let find = FindQuery::new(selectors).fields(fields).limit(50000);
    println!("find: {:?}", find);
    let r = db.find_batched(find, tx, 5000, 0).await;
    println!("ret: {:?}", r);

    let db2 = client.db(&args.db).await?;
    let mut chunks: Vec<Vec<Vec<String>>> = Vec::new();

    while let Some(all_docs) = rx.recv().await {
        println!("Received {} docs", all_docs.total_rows);
        let mut docs: Vec<Vec<String>> = Vec::new();
        for r in all_docs.rows {
            let _id = r["_id"].as_str().unwrap().to_string();
            let _rev = r["_rev"].as_str().unwrap().to_string();
            println!("{}", &_id);
            let v = vec![_id, _rev];

            docs.push(v);
        }
        chunks.push(docs);
    }

    if chunks.is_empty() {
        println!("No documents matched the selector");
        return Ok(());
    }

    let mut futures = vec![worker(db2.clone(), chunks[0].clone())];
    for (i, chunk) in chunks.into_iter().enumerate() {
        if i > 0 {
            let t = worker(db2.clone(), chunk);
            futures.push(t);
        }
    }
    join_all(futures).await;
    println!("....finished");

    Ok(())
}

pub async fn save_all_server_design(
    config: &AppConfig,
    base_dir: &std::path::Path,
    server_name: &str,
    pending: Option<Arc<Mutex<Vec<String>>>>,
    ctx: Option<eframe::egui::Context>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let report = |msg: String, pending: &Option<Arc<Mutex<Vec<String>>>>, ctx: &Option<eframe::egui::Context>| {
        println!("{msg}");
        if let Some(p) = pending {
            if let Ok(mut q) = p.lock() {
                q.push(msg);
            }
        }
        if let Some(c) = ctx {
            c.request_repaint();
        }
    };

    report("Connecting to server...".to_string(), &pending, &ctx);
    let client = Client::new(&config.host, &config.user, &config.password)
        .map_err(|e| -> Box<dyn Error + Send + Sync> { e.to_string().into() })?;
    let dbs = client
        .list_dbs()
        .await
        .map_err(|e| -> Box<dyn Error + Send + Sync> { e.to_string().into() })?;

    let safe_server = server_name
        .replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "_")
        .trim()
        .to_string();
    let server_dir = base_dir.join(if safe_server.is_empty() { "server" } else { &safe_server });
    report(format!("Saving designs to {}", server_dir.display()), &pending, &ctx);

    let total_dbs = dbs.len();
    for (idx, dbname) in dbs.into_iter().enumerate() {
        if dbname.starts_with('_') {
            continue;
        }
        report(
            format!("[{}/{}] Saving designs: {}", idx + 1, total_dbs, dbname),
            &pending,
            &ctx,
        );
        let mut config2 = config.clone();
        config2.database = dbname;
        save_all_design(&config2, &server_dir, pending.clone(), ctx.clone()).await?;
    }
    report("Save all Designs finished.".to_string(), &pending, &ctx);
    Ok(())
}

pub async fn save_all_design(
    config: &AppConfig,
    base_dir: &std::path::Path,
    pending: Option<Arc<Mutex<Vec<String>>>>,
    ctx: Option<eframe::egui::Context>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let report = |msg: String, pending: &Option<Arc<Mutex<Vec<String>>>>, ctx: &Option<eframe::egui::Context>| {
        println!("{msg}");
        if let Some(p) = pending {
            if let Ok(mut q) = p.lock() {
                q.push(msg);
            }
        }
        if let Some(c) = ctx {
            c.request_repaint();
        }
    };

    let client = Client::new(&config.host, &config.user, &config.password)
        .map_err(|e| -> Box<dyn Error + Send + Sync> { e.to_string().into() })?;
    let db = client
        .db(&config.database)
        .await
        .map_err(|e| -> Box<dyn Error + Send + Sync> { e.to_string().into() })?;

    let mut o = QueryParams::default();
    o.start_key = Some("_design".to_string());
    o.end_key = Some("_design0".to_string());
    o.include_docs = Some(true);

    let mut collections = db
        .query_many_all_docs(QueriesParams::new(vec![o]))
        .await
        .map_err(|e| -> Box<dyn Error + Send + Sync> { e.to_string().into() })?;

    let safe_db = config.database.replace(['/', '\\'], "_");
    let db_dir = base_dir.join(&safe_db);
    std::fs::create_dir_all(&db_dir)
        .map_err(|e| -> Box<dyn Error + Send + Sync> { e.to_string().into() })?;

    let mut saved_count: usize = 0;
    if let Some(a) = collections.iter_mut().next() {
        for i in a.rows.clone() {
            if let Some(doc) = i.doc {
                let mut j = json::parse(&doc.to_string()).unwrap();
                j.remove("_rev");
                let safe_id = j["_id"].to_string().replace(['/', '\\'], "__");
                let filename = db_dir.join(format!("{safe_id}.json"));
                let data = j.dump();
                tokio::fs::write(&filename, data)
                    .await
                    .map_err(|e| -> Box<dyn Error + Send + Sync> { e.to_string().into() })?;
                saved_count += 1;
            }
        }
    }
    report(
        format!("  {}: {} design documents saved.", config.database, saved_count),
        &pending,
        &ctx,
    );

    Ok(())
}

pub async fn save_all_server_dbs(
    base_path: &std::path::Path,
    server_name: &str,
    config: &AppConfig,
    pending: Option<Arc<Mutex<Vec<String>>>>,
    ctx: Option<eframe::egui::Context>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let report = |msg: String, pending: &Option<Arc<Mutex<Vec<String>>>>, ctx: &Option<eframe::egui::Context>| {
        println!("{msg}");
        if let Some(p) = pending {
            if let Ok(mut q) = p.lock() {
                q.push(msg);
            }
        }
        if let Some(c) = ctx {
            c.request_repaint();
        }
    };

    report("Connecting to server...".to_string(), &pending, &ctx);
    let client = Client::new(&config.host, &config.user, &config.password)
        .map_err(|e| -> Box<dyn Error + Send + Sync> { e.to_string().into() })?;
    let dbs = client
        .list_dbs()
        .await
        .map_err(|e| -> Box<dyn Error + Send + Sync> { e.to_string().into() })?;
    report(format!("Found {} databases.", dbs.len()), &pending, &ctx);

    let safe_server = server_name
        .replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "_")
        .trim()
        .to_string();
    let server_dir = base_path.join(if safe_server.is_empty() { "server" } else { &safe_server });
    report(format!("Saving to {}", server_dir.display()), &pending, &ctx);

    let total_dbs = dbs.len();
    for (idx, dbname) in dbs.into_iter().enumerate() {
        report(format!("[{}/{}] Saving database: {}", idx + 1, total_dbs, dbname), &pending, &ctx);
        let db = client
            .db(&dbname)
            .await
            .map_err(|e| -> Box<dyn Error + Send + Sync> { e.to_string().into() })?;
        let safe_db = dbname.replace(['/', '\\'], "_");
        let db_dir = server_dir.join(&safe_db);
        std::fs::create_dir_all(&db_dir)
            .map_err(|e| -> Box<dyn Error + Send + Sync> { e.to_string().into() })?;

        let limit = 1000u64;
        let mut bookmark: Option<String> = None;
        let mut saved_count: usize = 0;
        loop {
            let mut find = FindQuery::find_all().limit(limit);
            if let Some(b) = &bookmark {
                find = find.bookmark(b);
            }
            let docs = db
                .find_raw(&find)
                .await
                .map_err(|e| -> Box<dyn Error + Send + Sync> { e.to_string().into() })?;
            if docs.rows.is_empty() {
                break;
            }
            for row in &docs.rows {
                let mut doc = row.clone();
                if let Some(obj) = doc.as_object_mut() {
                    obj.remove("_rev");
                }
                let id = doc["_id"].as_str().unwrap_or("unknown").to_string();
                let safe_id = id.replace(['/', '\\'], "__");
                let filename = db_dir.join(format!("{safe_id}.json"));
                let data = serde_json::to_string_pretty(&doc)
                    .map_err(|e| -> Box<dyn Error + Send + Sync> { e.to_string().into() })?;
                tokio::fs::write(&filename, data)
                    .await
                    .map_err(|e| -> Box<dyn Error + Send + Sync> { e.to_string().into() })?;
                saved_count += 1;
            }
            if saved_count % 1000 == 0 || docs.rows.len() < limit as usize {
                report(format!("  {}: {} documents saved so far...", dbname, saved_count), &pending, &ctx);
            }
            bookmark = docs.bookmark.clone();
            if docs.rows.len() < limit as usize {
                break;
            }
            if bookmark.is_none() {
                break;
            }
        }
        report(format!("Completed {}: {} documents saved.", dbname, saved_count), &pending, &ctx);
    }
    report("Save all Docs finished.".to_string(), &pending, &ctx);
    Ok(())
}
