#!/bin/sh

# Upload the site to the server

# where rclone should put remote files
RCLONE_DEST=${RCLONE_DEST:-linode:/s.pommicket.com/jigsaw}
# server user+hostname
REMOTE=${REMOTE:-jigsaw@pommicket.com}
for file in *.html *.js *.css *.mp3 *.png; do
	echo "upload $file"
	rclone copy --s3-acl=public-read $file $RCLONE_DEST || exit 1
done

# if static-only argument is given, exit now
printf '%s' "$@" | grep -q 'static-only' && exit 0

echo 'Copying over server files…'
tar czf server.tar.gz $(git ls-files server) || exit 1
scp server.tar.gz $REMOTE: || exit 1
rm server.tar.gz
ssh $REMOTE <<EOF

cd
tar xf server.tar.gz || exit 1
rm server.tar.gz
echo 'Updating rust…'
rustup update stable
cd server/src
echo 'Building server…'
cargo build --release || exit 1
echo 'Restarting server…'
sudo systemctl restart jigsaw-server.service || exit 1

EOF

cd ..
echo 'Done!'
