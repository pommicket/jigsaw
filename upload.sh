#!/bin/sh
for file in *.html *.js *.css *.mp3; do
	echo "upload $file"
	rclone copy --s3-acl=public-read $file linode:/s.pommicket.com/jigsaw
done
