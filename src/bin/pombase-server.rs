#![feature(plugin)]
#![plugin(rocket_codegen)]

extern crate getopts;
extern crate rocket;
extern crate serde_json;
#[macro_use] extern crate rocket_contrib;

extern crate pombase;

#[cfg(test)] mod tests;

use std::sync::Mutex;
use std::process;
use std::env;

use getopts::Options;

use rocket_contrib::{JSON, Value};

use pombase::api::query::Query;
use pombase::api::result::Result;
use pombase::api::query_exec::QueryExec;
use pombase::api::server_data::ServerData;

#[post("/query", data="<q>", format = "application/json")]
fn query_post(q: JSON<Query>, state: rocket::State<Mutex<QueryExec>>) -> Option<JSON<Result>> {
    let query_exec = state.lock().expect("failed to lock");
    Some(JSON(query_exec.exec(&q.into_inner())))
}

#[get ("/reload")]
fn reload(state: rocket::State<Mutex<QueryExec>>) {
    let mut query_exec = state.lock().expect("failed to lock");
    print!("reloading ...\n");
    query_exec.reload();
}


#[error(404)]
fn not_found() -> JSON<Value> {
    JSON(json!({
        "status": "error",
        "reason": "Resource was not found."
    }))
}

const PKG_NAME: &str = env!("CARGO_PKG_NAME");
const VERSION: &str = env!("CARGO_PKG_VERSION");

fn print_usage(program: &str, opts: Options) {
    let brief = format!("Usage: {} [options]", program);
    print!("{}", opts.usage(&brief));
}

fn main() {
    print!("{} v{}\n", PKG_NAME, VERSION);

    let args: Vec<String> = env::args().collect();
    let mut opts = Options::new();

    opts.optflag("h", "help", "print this help message");
    opts.optopt("m", "search-maps", "Search data", "MAPS_JSON_FILE");

    let matches = match opts.parse(&args[1..]) {
        Ok(m) => m,
        Err(f) => panic!("Invalid options\n{}", f)
    };

    let program = args[0].clone();

    if matches.opt_present("help") {
        print_usage(&program, opts);
        process::exit(0);
    }

    if !matches.opt_present("search-maps") {
        print!("no --search-maps|-m option\n");
        print_usage(&program, opts);
        process::exit(1);
    }

    let search_maps_filename = matches.opt_str("m").unwrap();
    println!("Reading maps ...");

    let server_data = ServerData::new(&search_maps_filename);
    let query_exec = QueryExec::new(server_data);

    println!("Starting server ...");
    rocket::ignite()
        .mount("/", routes![query_post, reload])
        .catch(errors![not_found])
        .manage(Mutex::new(query_exec))
        .launch();
}
