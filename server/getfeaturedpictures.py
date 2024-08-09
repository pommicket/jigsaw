#!/usr/bin/env python3
import requests
import json
import time
import urllib.parse

headers = {'Accept-Encoding':'gzip', 'User-Agent': 'contact pommicket+jigsaw @ gmail.com '}
def make_file_request(cmcontinue):
	URL = 'https://commons.wikimedia.org/w/api.php?action=query&format=json&list=categorymembers&cmlimit=500&cmtitle=Category:Featured_pictures_on_Wikimedia_Commons&cmtype=file&cmprop=title&maxlag=5'
	while True:
		time.sleep(1)
		url = URL + '&cmcontinue=' + cmcontinue if cmcontinue else URL
		response = requests.get(url,headers=headers)
		if 'X-Database-Lag' in response.headers:
			time.sleep(5)
		break
	return json.loads(response.text)

def make_url_request(images):
	while True:
		time.sleep(1)
		url = 'https://commons.wikimedia.org/w/api.php?action=query&format=json&maxlag=5&prop=imageinfo&iiprop=url&titles=' + urllib.parse.quote('|'.join(images))
		response = requests.get(url,headers=headers)
		if 'X-Database-Lag' in response.headers:
			time.sleep(5)
		break
	return json.loads(response.text)

def get_files():
	with open('featuredpictures_files.txt', 'w') as f:
		cmcontinue = ''
		count = 0
		while cmcontinue is not None:
			print(count,'files gathered')
			response = make_file_request(cmcontinue)
			if 'query' in response and 'categorymembers' in response['query']:
				members = response['query']['categorymembers']
				f.write(''.join(page['title'] + '\n' for page in members))
				count += len(members)
			else:
				print('no categorymembers?')
				print(response)
			if 'continue' in response and 'cmcontinue' in response['continue']:
				cmcontinue = response['continue']['cmcontinue']
			else:
				cmcontinue = None
				print('no continue! done probably')
				break
			
def get_urls():
	with open('featuredpictures_files.txt', 'r') as f:
		files = [line.strip() for line in f]
	with open('featuredpictures.txt', 'w') as f:
		for i in range(0, len(files), 30):
			print('got URLs for',i,'files')
			batch = files[i:min(len(files), i + 30)]
			response = make_url_request(batch)
			f.write(''.join(page['imageinfo'][0]['url'] + '\n' for page in response['query']['pages'].values()))
get_files()
get_urls()
