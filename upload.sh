#!/bin/sh
for file in *.html *.js *.css *.mp3; do
	echo "upload $file"
	rclone copy --s3-acl=public-read $file linode:/s.pommicket.com/jigsaw || exit 1
done
cd server
cargo build --release || exit 1
scp target/release/jigsaw-server featuredpictures.txt potd.py getfeaturedpictures.py jigsaw@pommicket.com:server/
cd ..
