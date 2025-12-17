# curl -X POST http://couchdb.salamander-jewelry.net/logger/_find -d '{"selector":{"logger":"SAPI"},"fields":["_id"],"execution_stats": true, "limit":100}'  -H 'Content-Type:application/json'
# cargo  run -- --nox 1 --db logger --delete by_key --key logger --value SAPI
