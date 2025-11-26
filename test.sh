curl -X POST http://couchdb.salamander-jewelry.net/logger/_find -d '{"selector":{"logger":"SAPI"},"fields":["_id"],"execution_stats": true, "limit":100}'  -H 'Content-Type:application/json'
