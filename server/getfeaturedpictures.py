#!/usr/bin/env python3
import requests
import json
import time
from urllib.parse import quote

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

def get_urls_of_images(images):
	while True:
		time.sleep(1)
		url = 'https://commons.wikimedia.org/w/api.php?action=query&format=json&maxlag=5&prop=imageinfo&iiprop=url&titles=' + quote('|'.join(images))
		response = requests.get(url,headers=headers)
		if 'X-Database-Lag' in response.headers:
			time.sleep(5)
		break
	response = json.loads(response.text)
	return {page['title']: page['imageinfo'][0]['url'] for page in response['query']['pages'].values()}
	
def get_featured_files():
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
			
def get_featured_urls():
	with open('featuredpictures_files.txt', 'r') as f:
		files = [line.strip() for line in f]
	with open('featuredpictures.txt', 'w') as f:
		for i in range(0, len(files), 30):
			print('got URLs for',i,'files')
			batch = files[i:min(len(files), i + 30)]
			urls = get_urls_of_images(batch)
			f.write(''.join(f'{urls[x]} https://commons.wikimedia.org/wiki/{quote(x)}\n' for x in batch))

if __name__ == '__main__':
	get_featured_files()
	get_featured_urls()
