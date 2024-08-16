#!/usr/bin/env python3
import requests
from xml.etree import ElementTree
from getfeaturedpictures import get_urls_of_images
from urllib.parse import unquote

headers = {'Accept-Encoding':'gzip', 'User-Agent': 'contact pommicket+jigsaw @ gmail.com '}

URL = 'https://commons.wikimedia.org/w/api.php?action=featuredfeed&feed=potd&feedformat=rss&maxlag=5'

response = requests.get(URL, headers=headers).text
xml = ElementTree.fromstring(response)
item = xml.findall('channel/item')[-1]
desc = item.find('description').text
start = desc.index('"/wiki/File:') + len('"/wiki/')
end = desc.index('"', start)
name_escaped = desc[start:end]
name = unquote(name_escaped)
url = get_urls_of_images([name])[name]
link = f'https://commons.wikimedia.org/wiki/{name_escaped}'
print(url, link)
