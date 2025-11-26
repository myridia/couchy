source $HOME/.cargo/env
#cargo watch --no-gitignore -w 'src/' -x 'run -- --nox 1'
#cargo watch --no-gitignore -w 'src/' -x 'run -- --nox 1 --save all_design'
#cargo watch --no-gitignore -w 'src/' -x 'run -- --nox 1 --save all_server_design'
#cargo watch --no-gitignore -w 'src/' -x 'run -- --nox 1 --delete orphans --master http://couchdb.salamander-jewelry.net --repl http://cb1.salamander-jewelry.com:5984 --database sl_usa_style'
cargo watch --no-gitignore -w 'src/' -x 'run -- --nox 1 --db logger --delete by_key --key logger --value SAPIF'






