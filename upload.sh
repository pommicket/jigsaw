#!/bin/sh

# Upload the site to the server

# where rclone should put remote files
RCLONE_DEST=${RCLONE_DEST:-linode:/s.pommicket.com/jigsaw}
# server user+hostname
REMOTE=${REMOTE:-jigsaw@pommicket.com}
for file in *.html *.js *.css *.mp3; do
	echo "upload $file"
	rclone copy --s3-acl=public-read $file $RCLONE_DEST || exit 1
done

# if static-only argument is given, exit now
printf '%s' "$@" | grep -q 'static-only' && exit 0

cd server
echo 'Building server…'
cargo build --release || exit 1
echo 'Stopping server…'
ssh $REMOTE sudo systemctl stop jigsaw-server.service  || exit 1
echo 'Copying server files…'
scp -C featuredpictures.txt potd.py getfeaturedpictures.py  target/release/jigsaw-server ${REMOTE}:server/  || exit 1
echo 'Restarting server…'
ssh $REMOTE sudo systemctl start jigsaw-server.service || exit 1
cd ..
echo 'Done!'
