'use strict';
window.addEventListener('load', function () {
	const ACTION_MOVE = 3;
	const ACTION_CONNECT = 4;
	const socket = new WebSocket(location.protocol === "file:" || location.hostname === "localhost" ? "ws://localhost:54472" : "wss://jigsaw.pommicket.com");
	const searchParams = new URL(location.href).searchParams;
	socket.binaryType = "arraybuffer";
	let puzzleSeed = Math.floor(Math.random() * 0x7fffffff);
	// direct URL to image file
	let imageUrl = searchParams.has('image') ? encodeURI(searchParams.get('image')) : undefined;
	// link to page with info about image (e.g. https://commons.wikimedia.org/wiki/File:Foo.jpg)
	let imageLink = imageUrl;
	let puzzleWidth, puzzleHeight;
	const roughPieceCount = parseInt(searchParams.get('pieces'));
	const getById = (id) => document.getElementById(id);
	const playArea = getById("play-area");
	const connectAudio = getById("connect-audio");
	const solveAudio = getById("solve-audio");
	const imageLinkElement = getById('image-link');
	const joinPuzzle = searchParams.get('join');
	const joinLink = getById('join-link');
	function setJoinLink(puzzleID) {
		const url = new URL(location.href);
		url.hash = '';
		joinLink.href = '?' + new URLSearchParams({
			join: puzzleID
		}).toString();
		joinLink.style.display = 'inline';
	}
	if (joinPuzzle) setJoinLink(joinPuzzle);
	let solved = false;
	const connectRadius = 10;
	let pieceZIndexCounter = 1;
	let draggingPiece = null;
	let nibSize;
	let pieceWidth;
	let pieceHeight;
	let multiplayer = !joinLink;
	let waitingForAck = 0;
	if (imageUrl && imageUrl.startsWith('http')) {
		// make sure we use https
		let url = new URL(imageUrl);
		url.protocol = 'https:';
		imageUrl = url.href;
	}
	const image = new Image();
	const draggingPieceLastPos = Object.preventExtensions({x: null, y: null});
	let randomSeed = 123456789;
	function setRandomSeed(to) {
		randomSeed = to;
		// randomize a little
		random();
		random();
	}
	function debugAddPoint(element, x, y, color, id) {
		if (!color) color = 'red';
		const point = document.createElement('div');
		point.classList.add('debug-point');
		point.style.left = (x + element.getBoundingClientRect().left) + 'px';
		point.style.top = (y + element.getBoundingClientRect().top) + 'px';
		point.style.backgroundColor = color;
		if (id !== undefined) point.dataset.id = id;
		document.body.appendChild(point);
	}
	function canonicalToScreenPos(canonical) {
		return {
			x: canonical.x * playArea.clientWidth,
			y: canonical.y  * playArea.clientHeight,
		};
	}
	function screenPosToCanonical(scr) {
		return {
			x: scr.x / playArea.clientWidth,
			y: scr.y / playArea.clientHeight,
		};
	}
	function deriveConnectedPiecePositions() {
		for (const piece of pieces) {
			piece.deriveConnectedPiecePositions();
		}
	}
	function setPieceSize(w, h) {
		pieceWidth = w;
		pieceHeight = h;
		nibSize = Math.min(pieceWidth / 4, pieceHeight / 4);
		for (const piece of pieces)
			piece.updatePieceSize();
		deriveConnectedPiecePositions();
		document.body.style.setProperty('--piece-width', (pieceWidth) + 'px');
		document.body.style.setProperty('--piece-height', (pieceHeight) + 'px');
		document.body.style.setProperty('--nib-size', (nibSize) + 'px');
		document.body.style.setProperty('--image-width', (pieceWidth * puzzleWidth) + 'px');
		document.body.style.setProperty('--image-height', (pieceHeight * puzzleHeight) + 'px');
	}
	function random() {
		// https://en.wikipedia.org/wiki/Linear_congruential_generator
		// this uses the "Microsoft Visual/Quick C/C++" constants because
		// they're small enough that we don't have to worry about Number.MAX_SAFE_INTEGER
		randomSeed = (214013 * randomSeed + 2531011) & 0x7fffffff;
		let x1 = randomSeed >> 16;
		randomSeed = (214013 * randomSeed + 2531011) & 0x7fffffff;
		let x2 = randomSeed >> 16;
		return (x1 << 15 | x2) * (1 / (1 << 30));
	}
	const TOP_IN = 0;
	const TOP_OUT = 1;
	const RIGHT_IN = 2;
	const RIGHT_OUT = 3;
	const BOTTOM_IN = 4;
	const BOTTOM_OUT = 5;
	const LEFT_IN = 6;
	const LEFT_OUT = 7;
	const pieces = [];
	function inverseOrientation(o) {
		switch (o) {
		case TOP_IN: return BOTTOM_OUT;
		case TOP_OUT: return BOTTOM_IN;
		case RIGHT_IN: return LEFT_OUT;
		case RIGHT_OUT: return LEFT_IN;
		case BOTTOM_IN: return TOP_OUT;
		case BOTTOM_OUT: return TOP_IN;
		case LEFT_IN: return RIGHT_OUT;
		case LEFT_OUT: return RIGHT_IN;
		}
		console.assert(false);
	}
	function connectPieces(piece1, piece2, interactive) {
		if (piece1.connectedComponent === piece2.connectedComponent) return false;
		if (interactive && piece1.connectedComponent.length < piece2.connectedComponent.length) {
			// always connect the smaller component to the larger component
			return connectPieces(piece2, piece1, interactive);
		}
		piece1.connectedComponent.push(...piece2.connectedComponent);
		const maxZIndex = Math.max(...piece1.connectedComponent.map((x) => parseInt(x.element.style.zIndex)));
		for (const piece of piece2.connectedComponent) {
			piece.connectedComponent = piece1.connectedComponent;
		}
		for (const piece of piece1.connectedComponent) {
			// update z-index to max in connected component
			piece.element.style.zIndex = maxZIndex;
		}
		if (interactive)
			deriveConnectedPiecePositions();
		if (!solved && piece1.connectedComponent.length === puzzleWidth * puzzleHeight) {
			solveAudio.play();
			solved = true;
		}
		return true;
	}
	class NibType {
		orientation;
		dx11;
		dy11;
		dx12;
		dy12;
		dx22;
		dy22;
		constructor(orientation) {
			console.assert(orientation >= 0 && orientation < 8);
			this.dx11 = 0;
			this.dy11 = 0;
			this.dx12 = 0;
			this.dy12 = 0;
			this.dx12 = 0;
			this.dy22 = 0;
			this.dx22 = 0;
			this.dy22 = 0;
			this.orientation = orientation;
		}
		inverse() {
			let inv = new NibType(inverseOrientation(this.orientation));
			inv.dx11 = -this.dx22;
			inv.dy11 = this.dy22;
			inv.dx12 = this.dx12;
			inv.dy12 = this.dy12;
			inv.dx22 = -this.dx11;
			inv.dy22 = this.dy11;
			return inv;
		}
		randomize() {
			const bendiness = 0.5;
			this.dx11 = (random() *  2 - 1) * bendiness;
			this.dy11 = (random() * 2 - 1) * bendiness;
			this.dx12 = (random() *  2 - 1) * bendiness;
			// this ensures base of nib is flat
			this.dy12 = 1;
			this.dx22 = (random() *  2 - 1) * bendiness;
			this.dy22 = (random() * 2 - 1) * bendiness;
			return this;
		}
		static random(orientation) {
			return new NibType(orientation).randomize();
		}
		path() {
			let xMul = this.orientation === BOTTOM_IN || this.orientation === LEFT_IN
				|| this.orientation === BOTTOM_OUT || this.orientation === LEFT_OUT ? -nibSize : nibSize;
			let yMul = this.orientation === RIGHT_IN || this.orientation === BOTTOM_IN
				|| this.orientation === TOP_OUT || this.orientation === LEFT_OUT ? -nibSize : nibSize;
			let dx11 = this.dx11 * xMul;
			let dy11 = (1 / 2 + this.dy11) * yMul;
			let dx12 = this.dx12 * xMul;
			let dy12 = this.dy12 * yMul;
			let dx22 = (1 / 2 + this.dx22) * xMul;
			let dy22 = (-1 / 2 + this.dy22) * yMul;
			let dx1 = (1 / 2) * xMul;
			let dy1 = yMul;
			let dx2 = (1 / 2) * xMul;
			let dy2 = -yMul;
			if (this.orientation === LEFT_IN
				|| this.orientation === RIGHT_IN
				|| this.orientation === LEFT_OUT
				|| this.orientation === RIGHT_OUT) {
				[dx11, dy11] = [dy11, dx11];
				[dx12, dy12] = [dy12, dx12];
				[dx22, dy22] = [dy22, dx22];
				[dx1, dy1] = [dy1, dx1];
				[dx2, dy2] = [dy2, dx2];
			}
			return `c${dx11} ${dy11} ${dx12} ${dy12} ${dx1} ${dy1}`
				+ ` s${dx22} ${dy22} ${dx2} ${dy2}`;
		}
	}
	class Piece {
		id;
		u;
		v;
		x;
		y;
		element;
		nibTypes;
		connectedComponent;
		needsServerUpdate;
		constructor(id, x, y, nibTypes) {
			this.id = id;
			this.x = x;
			this.y = y;
			this.nibTypes = nibTypes;
			this.needsServerUpdate = true;
			this.connectedComponent = [this];
			const element = this.element = document.createElement('div');
			element.classList.add('piece');
			const outerThis = this;
			element.addEventListener('mousedown', function(e) {
				if (e.button !== 0) return;
				draggingPiece = outerThis;
				draggingPieceLastPos.x = e.clientX;
				draggingPieceLastPos.y = e.clientY;
				this.style.zIndex = ++pieceZIndexCounter;
				this.style.cursor = 'none';
			});
			element.style.zIndex = 0; // default zIndex
			this.updatePosition();
			const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
			this.element.appendChild(svg);
			this.updatePieceSize();
			// disable animation during initialization
			this.setAnimate(false);
			playArea.appendChild(element);
		}
		updatePieceSize() {
			const svg = this.element.querySelector('svg');
			const clipPath = this.getClipPath();
			this.element.style.clipPath = `path("${clipPath}")`;
			svg.setAttribute('width', pieceWidth + 2 * nibSize);
			svg.setAttribute('height', pieceHeight + 2 * nibSize);
			svg.setAttribute('viewBox', `0 0 ${pieceWidth + 2 * nibSize} ${pieceHeight + 2 * nibSize}`);
			svg.innerHTML = `<path d="${clipPath}" stroke-width="${pieceWidth < 50 ? 1 : 2}" stroke="black" fill="none" />`;
			this.element.style.backgroundPositionX = (nibSize - this.col() * pieceWidth) + 'px';
			this.element.style.backgroundPositionY = (nibSize - this.row() * pieceHeight) + 'px';
		}
		col() {
			return this.id % puzzleWidth;
		}
		row() {
			return Math.floor(this.id / puzzleWidth);
		}
		updatePosition() {
			this.element.style.left = (100 * this.x) + '%';
			this.element.style.top = (100 * this.y) + '%';
		}
		boundingBox() {
			const pos = canonicalToScreenPos(this);
			return Object.preventExtensions({
				left: pos.x, top: pos.y, right: pos.x + pieceWidth + 2 * nibSize, bottom: pos.y + pieceHeight + 2 * nibSize
			});
		}
		deriveConnectedPiecePositions() {
			if (this === this.connectedComponent[0]) {
				const myRow = this.row();
				const myCol = this.col();
				for (const piece of this.connectedComponent) {
					if (piece === this) continue;
					const row = piece.row();
					const col = piece.col();
					piece.x = (col - myCol) * pieceWidth / playArea.clientWidth + this.x;
					piece.y = (row - myRow) * pieceHeight / playArea.clientHeight + this.y;
					piece.updatePosition();
				}
			}
		}
		setAnimate(enabled) {
			if (enabled) {
				this.element.classList.remove('no-animation');
			} else {
				this.element.classList.add('no-animation');
			}
		}
		getClipPath() {
			const nibTypes = this.nibTypes;
			let shoulderWidth = (pieceWidth - nibSize) / 2;
			let shoulderHeight = (pieceHeight - nibSize) / 2;
			let clipPath = [];
			clipPath.push(`M${nibSize} ${nibSize}`);
			clipPath.push(`l${shoulderWidth} 0`);
			if (nibTypes[0]) {
				clipPath.push(nibTypes[0].path());
			}
			clipPath.push(`L${pieceWidth + nibSize} ${nibSize}`);
			clipPath.push(`l0 ${shoulderHeight}`);
			if (nibTypes[1]) {
				clipPath.push(nibTypes[1].path());
			}
			clipPath.push(`L${pieceWidth + nibSize} ${pieceHeight + nibSize}`);
			clipPath.push(`l-${shoulderWidth} 0`);
			if (nibTypes[2]) {
				clipPath.push(nibTypes[2].path());
			}
			clipPath.push(`L${nibSize} ${pieceHeight + nibSize}`);
			clipPath.push(`l0 -${shoulderHeight}`);
			if (nibTypes[3]) {
				clipPath.push(nibTypes[3].path());
			}
			clipPath.push(`L${nibSize} ${nibSize}`);
			return clipPath.join(' ');
		}
	}
	window.addEventListener('mouseup', function() {
		if (draggingPiece) {
			let anyConnected = false;
			for (const piece of draggingPiece.connectedComponent) {
				piece.setAnimate(true);
				piece.element.style.zIndex = pieceZIndexCounter;
				if (solved) break;
				piece.needsServerUpdate = true;
				const col = piece.col();
				const row = piece.row();
				const bbox = piece.boundingBox();
				for (const [nx, ny] of [[0, -1], [0, 1], [1, 0], [-1, 0]]) {
					if (col + nx < 0 || col + nx >= puzzleWidth
						|| row + ny < 0 || row + ny >= puzzleHeight) {
							continue;
					}
					let neighbour = pieces[piece.id + nx + ny * puzzleWidth];
					if (neighbour.connectedComponent === piece.connectedComponent)
						continue;
					let neighbourBBox = neighbour.boundingBox();
					let keyPointMe = [nx === -1 ? bbox.left + nibSize : bbox.right - nibSize,
						ny === -1 ? bbox.top + nibSize : bbox.bottom - nibSize];
					let keyPointNeighbour = [nx === 1 ?  neighbourBBox.left + nibSize : neighbourBBox.right - nibSize,
						ny === 1 ? neighbourBBox.top + nibSize : neighbourBBox.bottom - nibSize];
					let diff = [keyPointMe[0] - keyPointNeighbour[0], keyPointMe[1] - keyPointNeighbour[1]];
					let sqDist = diff[0] * diff[0] + diff[1] * diff[1];
					if (sqDist < connectRadius * connectRadius) {
						anyConnected = true;
						connectPieces(piece, neighbour, true);
						if (multiplayer) {
							socket.send(new Uint32Array([ACTION_CONNECT, piece.id, neighbour.id]));
						}
					}
				}
			}
			draggingPiece.element.style.removeProperty('cursor');
			draggingPiece = null;
			if (anyConnected)
				connectAudio.play();
		}
	});
	window.addEventListener('mousemove', function(e) {
		if (draggingPiece) {
			let dx = (e.clientX - draggingPieceLastPos.x) / playArea.clientWidth;
			let dy = (e.clientY - draggingPieceLastPos.y) / playArea.clientHeight;
			for (const piece of draggingPiece.connectedComponent) {
				// ensure pieces don't go past left edge
				dx = Math.max(dx, 0.0001 - piece.x);
				dy = Math.max(dy, 0.0001 - piece.y);
				// ensure pieces don't go past right edge
				dx = Math.min(dx, 1.5 - piece.x);
				dy = Math.min(dy, 1.5 - piece.y);
			}
			for (const piece of draggingPiece.connectedComponent) {
				piece.element.style.zIndex = pieceZIndexCounter;
				piece.setAnimate(false);
				piece.x += dx;
				piece.y += dy;
				piece.updatePosition();
			}
			draggingPieceLastPos.x = e.clientX;
			draggingPieceLastPos.y = e.clientY;
		}
	});
	function loadImage() {
		document.body.style.setProperty('--image', `url("${imageUrl}")`);
		imageLinkElement.style.visibility = 'visible';
		imageLinkElement.href = imageLink;
		const promise = new Promise((resolve) => {
			image.addEventListener('load', function () {
				resolve();
			});
		});
		image.src = imageUrl;
		return promise;
	}
	function updateConnectivity(connectivity) {
		console.assert(connectivity.length === pieces.length);
		let anyConnected = false;
		for (let i = 0; i < pieces.length; i++) {
			anyConnected |= connectPieces(pieces[i], pieces[connectivity[i]], false);
		}
		for (let i = 0; i < pieces.length; i++) {
			const piece = pieces[i];
			const connectedComponent = piece.connectedComponent;
			if (i === connectivity[i] && piece !== connectedComponent[0]) {
				// ensure piece i comes first in my connected component if i === connectivity[i]
				const index = connectedComponent.indexOf(piece);
				connectedComponent.splice(index, 1);
				connectedComponent.unshift(piece);
				console.log(connectedComponent[0]);
			}
		}
		deriveConnectedPiecePositions();
		if (anyConnected) connectAudio.play();
	}
	function createPieces() {
		if (playArea.clientWidth / puzzleWidth < playArea.clientHeight / puzzleHeight) {
			pieceWidth = 0.8 * playArea.clientWidth / puzzleWidth;
			pieceHeight = pieceWidth * (puzzleWidth / puzzleHeight) * (image.height / image.width);
		} else {
			pieceHeight = 0.8 * playArea.clientHeight / puzzleHeight;
			pieceWidth = pieceHeight * (puzzleHeight / puzzleWidth) * (image.width / image.height);
		}
		// ensure full puzzle doesn't take up too much screen space
		while (pieceWidth * puzzleWidth * pieceHeight * puzzleHeight > Math.max(1000, 0.5 * playArea.clientWidth * playArea.clientHeight)) {
			pieceWidth *= 0.9;
			pieceHeight *= 0.9;
		}
		setPieceSize(pieceWidth, pieceHeight);
		for (let v = 0; v < puzzleHeight; v++) {
			for (let u = 0; u < puzzleWidth; u++) {
				let nibs = [null, null, null, null];
				let id = pieces.length;
				if (v > 0) nibs[0] = pieces[id - puzzleWidth].nibTypes[2].inverse();
				if (u < puzzleWidth - 1) {
					nibs[1] = NibType.random(Math.floor(random() * 2) ? RIGHT_IN : RIGHT_OUT);
				}
				if (v < puzzleHeight - 1) {
					nibs[2] = NibType.random(Math.floor(random() * 2) ? BOTTOM_IN : BOTTOM_OUT);
				}
				if (u > 0) nibs[3] = pieces[id - 1].nibTypes[1].inverse();
				pieces.push(new Piece(id, 0, 0, nibs));
			}
		}
	}
	async function createPuzzle() {
		await loadImage();
		if (isNaN(roughPieceCount) || roughPieceCount < 10 || roughPieceCount > 1000) {
			// TODO : better error reporting
			console.error('bad piece count');
			return;
		}
		let bestWidth = 1;
		let bestDiff = Infinity;
		function heightFromWidth(w) {
			return Math.min(255, Math.max(2, Math.round(w * image.height / image.width)));
		}
		for (let width = 2; width < 256; width++) {
			const height = heightFromWidth(width);
			if (width * height > 1000) break;
			const diff = Math.abs(width * height - roughPieceCount);
			if (diff < bestDiff) {
				bestDiff = diff;
				bestWidth = width;
			}
		}
		puzzleWidth = bestWidth;
		puzzleHeight = heightFromWidth(puzzleWidth);
		getById('host-multiplayer').style.display = 'inline-block';
		setRandomSeed(puzzleSeed);
		createPieces();
		// a bit janky, but it stops the pieces from animating to their starting positions
		setTimeout(() => {
			for (const piece of pieces) {
				piece.setAnimate(true);
			}
		}, 100);
	}
	async function initPuzzle(payload) {
		const data = new Uint8Array(payload, payload.length);
		puzzleSeed = new Uint32Array(payload, 8, 1)[0];
		setRandomSeed(puzzleSeed);
		puzzleWidth = data[12];
		puzzleHeight = data[13];
		const imageUrlOffset = 14;
		const imageUrlLen = new Uint8Array(payload, imageUrlOffset, data.length - imageUrlOffset).indexOf(0);
		const imageUrlBytes = new Uint8Array(payload, imageUrlOffset, imageUrlLen);
		let piecePositionsOffset = imageUrlOffset + imageUrlLen + 1;
		piecePositionsOffset = Math.floor((piecePositionsOffset + 7) / 8) * 8; // align to 8 bytes
		const piecePositions = new Float32Array(payload, piecePositionsOffset, puzzleWidth * puzzleHeight * 2);
		const connectivityOffset = piecePositionsOffset + piecePositions.length * 4;
		const connectivity = new Uint16Array(payload, connectivityOffset, puzzleWidth * puzzleHeight);
		const parts = new TextDecoder().decode(imageUrlBytes).split(' ');
		imageUrl = parts[0];
		imageLink = parts.length > 1 ? parts[1] : parts[0];
		await loadImage();
		createPieces();
		for (let id = 0; id < pieces.length; id++) {
			if (id !== connectivity[id]) continue; // only set one piece positions per piece group
			pieces[id].x = piecePositions[2 * connectivity[id]];
			pieces[id].y = piecePositions[2 * connectivity[id] + 1];
			pieces[id].updatePosition();
		}
		updateConnectivity(connectivity);
		// a bit janky, but it stops the pieces from animating to their starting positions
		setTimeout(() => {
			for (const piece of pieces) {
				piece.setAnimate(true);
			}
		}, 100);
	}
	async function hostPuzzle() {
		socket.send(`new ${puzzleWidth} ${puzzleHeight} ${imageUrl};${imageLink} ${puzzleSeed}`);
		multiplayer = true;
	}
	function applyUpdate(update) {
		const piecePositions = new Float32Array(update, 8, puzzleWidth * puzzleHeight * 2);
		const connectivity = new Uint16Array(update, 8 + piecePositions.length * 4, puzzleWidth * puzzleHeight);
		updateConnectivity(connectivity);
		for (let i = 0; i < pieces.length; i++) {
			// only receive the position of one piece per piece group
			if (connectivity[i] !== i) continue;
			const piece = pieces[i];
			if (piece.needsServerUpdate) continue;
			if (draggingPiece && draggingPiece.connectedComponent === piece.connectedComponent) continue;
			const newPos = {x: piecePositions[2 * i], y: piecePositions[2 * i + 1]};
			const diff = [newPos.x - piece.x, newPos.y - piece.y];
			const minRadius = 0.01; // don't bother moving less than 1%
			if (diff[0] * diff[0] + diff[1] * diff[1] < minRadius * minRadius) continue;
			piece.x = newPos.x;
			piece.y = newPos.y;
			piece.updatePosition();
			// derive all other pieces' position in this connected component from piece.
			for (const other of piece.connectedComponent) {
				if (other === piece) continue;
				other.x = piece.x + (other.col() - piece.col()) * pieceWidth / playArea.clientWidth;
				other.y = piece.y + (other.row() - piece.row()) * pieceHeight / playArea.clientHeight;
				other.updatePosition();
			}
		}
	}
	function sendServerUpdate() {
		// send update to server
		if (!multiplayer) return;
		if (waitingForAck) return; // last update hasn't been acknowledged yet
		let actions = new Uint32Array(pieces.length * 4 + 1);
		const pos = new Float32Array(2);
		const posU8 = new Uint8Array(pos.buffer);
		let i = 0;
		let messageID = 0x12345678; // message ID for regular updates
		actions[i++] = messageID;
		for (const piece of pieces) {
			if (!piece.needsServerUpdate) continue;
			actions[i++] = ACTION_MOVE;
			actions[i++] = piece.id;
			pos[0] = piece.x;
			pos[1] = piece.y;
			new Uint8Array(actions.buffer, 4 * i, 8).set(posU8);
			i += 2;
		}
		if (i > 1) {
			waitingForAck = messageID;
			socket.send(new Uint32Array(actions.buffer, 0, i));
		}
	}
	let waitingForServerToGiveUsImageUrl = false;
	socket.addEventListener('open', async () => {
		if (joinPuzzle) {
			socket.send(`join ${joinPuzzle}`);
		} else if (imageUrl.startsWith('http')) {
			createPuzzle();
		} else if (imageUrl === 'randomFeaturedWikimedia') {
			socket.send('randomFeaturedWikimedia');
			waitingForServerToGiveUsImageUrl = true;
		} else if (imageUrl === 'wikimediaPotd') {
			socket.send('wikimediaPotd');
			waitingForServerToGiveUsImageUrl = true;
		} else {
			// TODO : better error reporting
			throw new Error("bad image URL");
		}
	});
	socket.addEventListener('message', async (e) => {
		if (typeof e.data === 'string') {
			if (e.data.startsWith('id: ')) {
				let puzzleID = e.data.split(' ')[1];
				sendServerUpdate(); // send piece positions
				const connectivityUpdate = [0 /* message ID */];
				for (const piece of pieces) {
					if (piece !== piece.connectedComponent[0]) {
						connectivityUpdate.push(ACTION_CONNECT, piece.connectedComponent[0].id, piece.id);
					}
				}
				socket.send(new Uint32Array(connectivityUpdate));
				history.pushState({}, null, `?join=${puzzleID}`);
				setJoinLink(puzzleID);
			} else if (e.data.startsWith('ack')) {
				const messageID = parseInt(e.data.split(' ')[1]);
				if (messageID === waitingForAck) {
					for (const piece of pieces) {
						piece.needsServerUpdate = false;
					}
					waitingForAck = 0;
				}
			} else if (waitingForServerToGiveUsImageUrl && e.data.startsWith('useImage ')) {
				waitingForServerToGiveUsImageUrl = false;
				const parts = e.data.substring('useImage '.length).split(' ');
				imageUrl = parts[0];
				imageLink = parts.length > 1 ? parts[1] : imageUrl;
				createPuzzle();
			} else if (e.data.startsWith('error ')) {
				const error = e.data.substring('error '.length);
				console.error(error); // TODO : better error handling
			}
		} else {
			const opcode = new Uint8Array(e.data, 0, 1)[0];
			if (opcode === 1 && !pieces.length) { // init puzzle
				await initPuzzle(e.data);
				setInterval(() => {
					if (multiplayer) {
						socket.send('poll');
					}
				}, 1000);
				setInterval(sendServerUpdate, 1000);
			} else if (opcode === 2) { // update puzzle
				applyUpdate(e.data);
			}
		}
	});
	const prevPlayAreaSize = Object.preventExtensions({width: playArea.clientWidth, height: playArea.clientHeight});
	function everyFrame() {
		if (prevPlayAreaSize.width !== playArea.clientWidth || prevPlayAreaSize.height !== playArea.clientHeight) {
			// disable animations while moving the pieces
			for (const piece of pieces) {
				piece.setAnimate(false);
			}
			// re-derive piece positions so connected pieces don't disconnect
			deriveConnectedPiecePositions();
			setTimeout(() => {
				for (const piece of pieces) {
					piece.setAnimate(true);
				}
			}, 100);
			prevPlayAreaSize.width = playArea.clientWidth;
			prevPlayAreaSize.height = playArea.clientHeight;
		}
		requestAnimationFrame(everyFrame);
	}
	getById('piece-size-plus').addEventListener('click', () => {
		setPieceSize(pieceWidth * 1.2, pieceHeight * 1.2);
	});
	getById('piece-size-minus').addEventListener('click', () => {
		setPieceSize(pieceWidth / 1.2, pieceHeight / 1.2);
	});
	getById('host-multiplayer').addEventListener('click', () => {
		hostPuzzle();
	});
	requestAnimationFrame(everyFrame);
});
