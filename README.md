# jigsaw

online cooperative jigsaw puzzles

https://s.pommicket.com/jigsaw/index.html

## about

this is the source code for a website where you can complete jigsaw puzzles with your friends.
it includes the ability to choose a random picture from the wikimedia commons,
or to use the picture of the day.

## running locally

you can run this website locally by installing [rust](https://rust-lang.org),
then running

```
cargo run --release
```

in the `server` directory. you will now be able to access the website via file:///.../jigsaw/index.html

## contributing

please contribute !
you can also report bugs you find or improvmenets you want to the [github issues page](https://github.com/pommicket/jigsaw/issues).

## hosting your own instance

this website consists of a backend written in rust, and a frontend which is just static files.

to host it, first create a `jigsaw` user (`useradd jigsaw`), and install `postgresql`. then run
`sudo -u postgres psql`, and enter

```
CREATE USER jigsaw;
GRANT ALL PRIVILEGES ON DATABASE jigsaw TO jigsaw;
GRANT CREATE ON SCHEMA public TO jigsaw;
\q
```

run `cargo build --release` in the `server` directory
either on the server or on your computer, and copy `target/release/jigsaw-server` to the jigsaw user's home directory (or any directory they
have access to).
now you can run it to start the backend, or create a systemd service to run it for you, e.g.:

```
[Unit]
Description=Jigsaw puzzle server
After=network.target

[Service]
User=jigsaw
WorkingDirectory=/home/jigsaw/server
ExecStart=/home/jigsaw/server/jigsaw-server
Type=simple
Restart=always

[Install]
WantedBy=default.target
RequiredBy=network.target
```

now for the front-end, first change the `WEBSOCKET_URL_REMOTE` constant in `game.js` to a subdomain of your domain.
run a proxy on your server to forward websocket traffic from that subdomain to port 54472 (this port can be configured in `server/src/main.rs`).
with apache2 this can be done as follows:

```
<VirtualHost *:443>
    ServerName <your subdomain>
    ProxyPass "/" "ws://localhost:54472/"
    ProxyPassReverse "/" "ws://localhost:54472/"
Include /etc/letsencrypt/options-ssl-apache.conf
SSLCertificateFile /etc/letsencrypt/live/<your domain>/fullchain.pem
SSLCertificateKeyFile /etc/letsencrypt/live/<your domain>/privkey.pem
</VirtualHost>
```

and that's it! you can update the files with the `upload.sh` script, modifying the `RCLONE_DEST` variable to point to where
your static files are stored, and `REMOTE` to point to the jigsaw user of your server. to use this script you will also need to add
the following to your sudoers file:

```
jigsaw ALL= NOPASSWD: /bin/systemctl start jigsaw-server.service
jigsaw ALL= NOPASSWD: /bin/systemctl stop jigsaw-server.service
jigsaw ALL= NOPASSWD: /bin/systemctl restart jigsaw-server.service
```

to allow the jigsaw user to manage the jigsaw-server service.

## license

WTFPL

