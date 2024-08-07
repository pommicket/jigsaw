'use strict';
window.addEventListener('load', function () {
	const socket = new WebSocket("ws://localhost:3000");
	socket.binaryType = "arraybuffer";
	let imageUrl = "https://upload.wikimedia.org/wikipedia/commons/0/09/Croatia_Opatija_Maiden_with_the_Seagull_BW_2014-10-10_10-35-13.jpg";
	let puzzleWidth = 4;
	let puzzleHeight = 3;
	socket.addEventListener('open', () => {
		socket.send(`new ${puzzleWidth} ${puzzleHeight} ${imageUrl}`);
	});
	socket.addEventListener('message', (e) => {
		console.log(e.data);
		setTimeout(() => socket.send('poll'), 1000);
	});
	const getById = (id) => document.getElementById(id);
	const playArea = getById("play-area");
	const connectAudio = getById("connect-audio");
	const solveAudio = getById("solve-audio");
	let solved = false;
	const connectRadius = 5;
	let pieceZIndexCounter = 1;
	let draggingPiece = null;
	let nibSize = 12;
	let pieceWidth = 70;
	let pieceHeight;
	document.body.style.setProperty('--image', `url("${imageUrl}")`);// TODO : escaping
	const image = new Image();
	image.src = imageUrl;
	const draggingPieceLastPos = Object.preventExtensions({x: null, y: null});
	var randomSeed = 123456789;
	function debugAddPoint(element, x, y, color, id) {
		if (!color) color = 'red';
		const point = document.createElement('div');
		point.classList.add('debug-point');
		console.log(element.getBoundingClientRect().left);
		point.style.left = (x + element.getBoundingClientRect().left) + 'px';
		point.style.top = (y + element.getBoundingClientRect().top) + 'px';
		point.style.backgroundColor = color;
		if (id !== undefined) point.dataset.id = id;
		document.body.appendChild(point);
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
	function connectPieces(piece1, piece2) {
		if (piece1.connectedComponent === piece2.connectedComponent) return;
		piece1.connectedComponent.push(...piece2.connectedComponent);
		for (const piece of piece2.connectedComponent) {
			piece.connectedComponent = piece1.connectedComponent;
		}
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
			this.dx11 = Math.floor((random() *  2 - 1)  * nibSize * bendiness);
			this.dy11 = Math.floor((random() * 2 - 1) * nibSize * bendiness);
			this.dx12 = Math.floor((random() *  2 - 1) * nibSize * bendiness);
			// this ensures base of nib is flat
			this.dy12 = nibSize;
			this.dx22 = Math.floor((random() *  2 - 1) * nibSize * bendiness);
			this.dy22 = Math.floor((random() * 2 - 1) * nibSize * bendiness);
			return this;
		}
		static random(orientation) {
			return new NibType(orientation).randomize();
		}
		path() {
			let xMul = this.orientation === BOTTOM_IN || this.orientation === LEFT_IN
				|| this.orientation === BOTTOM_OUT || this.orientation === LEFT_OUT ? -1 : 1;
			let yMul = this.orientation === RIGHT_IN || this.orientation === BOTTOM_IN
				|| this.orientation === TOP_OUT || this.orientation === LEFT_OUT ? -1 : 1;
			let dx11 = this.dx11 * xMul;
			let dy11 = (nibSize / 2 + this.dy11) * yMul;
			let dx12 = this.dx12 * xMul;
			let dy12 = this.dy12 * yMul;
			let dx22 = (nibSize / 2 + this.dx22) * xMul;
			let dy22 = (-nibSize / 2 + this.dy22) * yMul;
			let dx1 = (nibSize / 2) * xMul;
			let dy1 = nibSize * yMul;
			let dx2 = (nibSize / 2) * xMul;
			let dy2 = -nibSize * yMul;
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
		constructor(id, u, v, x, y, nibTypes) {
			this.id = id;
			this.x = x;
			this.y = y;
			this.u = u;
			this.v = v;
			this.connectedComponent = [this];
			const element = this.element = document.createElement('div');
			element.classList.add('piece');
			const outerThis = this;
			element.addEventListener('mousedown', function(e) {
				if (e.button !== 0) return;
				draggingPiece = outerThis;
				draggingPieceLastPos.x = e.clientX;
				draggingPieceLastPos.y = e.clientY;
				this.style.zIndex = pieceZIndexCounter++;
				this.style.cursor = 'none';
			});
			this.updateUV();
			this.updatePosition();
			const debugCurves = false;//display bezier control points for debugging
			if (debugCurves)
				playArea.appendChild(element);
			this.nibTypes = nibTypes;
			const clipPath = this.getClipPath();
			this.element.style.clipPath = `path("${clipPath}")`;
			const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
			svg.setAttribute('width', pieceWidth + 2 * nibSize);
			svg.setAttribute('height', pieceHeight + 2 * nibSize);
			svg.setAttribute('viewBox', `0 0 ${pieceWidth + 2 * nibSize} ${pieceHeight + 2 * nibSize}`);
			svg.innerHTML = `<path d="${clipPath}" stroke-width="1" stroke="black" fill="none" />`;
			this.element.appendChild(svg);
			if (!debugCurves)
				playArea.appendChild(element);
		}
		updateUV() {
			this.element.style.backgroundPositionX = (nibSize - this.u) + 'px';
			this.element.style.backgroundPositionY = (nibSize - this.v) + 'px';
		}
		updatePosition() {
			this.element.style.left = this.x + 'px';
			this.element.style.top = this.y + 'px';
		}
	}
	window.addEventListener('mouseup', function() {
		if (draggingPiece) {
			let anyConnected = false;
			for (const piece of draggingPiece.connectedComponent) {
				if (solved) break;
				const col = piece.id % puzzleWidth;
				const row = Math.floor(piece.id / puzzleWidth);
				const bbox = piece.element.getBoundingClientRect();
				for (const [nx, ny] of [[0, -1], [0, 1], [1, 0], [-1, 0]]) {
					if (col + nx < 0 || col + nx >= puzzleWidth
						|| row + ny < 0 || row + ny >= puzzleHeight) {
							continue;
					}
					let neighbour = pieces[piece.id + nx + ny * puzzleWidth];
					if (neighbour.connectedComponent === piece.connectedComponent)
						continue;
					let neighbourBBox = neighbour.element.getBoundingClientRect();
					let keyPointMe = [nx === -1 ? bbox.left + nibSize : bbox.right - nibSize,
						ny === -1 ? bbox.top + nibSize : bbox.bottom - nibSize];
					let keyPointNeighbour = [nx === 1 ?  neighbourBBox.left + nibSize : neighbourBBox.right - nibSize,
						ny === 1 ? neighbourBBox.top + nibSize : neighbourBBox.bottom - nibSize];
					let diff = [keyPointMe[0] - keyPointNeighbour[0], keyPointMe[1] - keyPointNeighbour[1]];
					let sqDist = diff[0] * diff[0] + diff[1] * diff[1];
					if (sqDist < connectRadius * connectRadius) {
						for (const piece2 of piece.connectedComponent) {
							piece2.x -= diff[0];
							piece2.y -= diff[1];
							piece2.updatePosition();
						}
						anyConnected = true;
						connectPieces(piece, neighbour);
					}
				}
			}
			if (!solved && draggingPiece.connectedComponent.length === puzzleWidth * puzzleHeight) {
				solveAudio.play();
				solved = true;
			}
			draggingPiece.element.style.removeProperty('cursor');
			draggingPiece = null;
			if (anyConnected)
				connectAudio.play();
		}
	});
	window.addEventListener('mousemove', function(e) {
		if (draggingPiece) {
			let dx = e.clientX - draggingPieceLastPos.x;
			let dy = e.clientY - draggingPieceLastPos.y;
			for (const piece of draggingPiece.connectedComponent) {
				piece.x += dx;
				piece.y += dy;
				piece.updatePosition();
			}
			draggingPieceLastPos.x = e.clientX;
			draggingPieceLastPos.y = e.clientY;
		}
	});
	
	image.addEventListener('load', function () {
		pieceHeight = pieceWidth * puzzleWidth * image.height / (puzzleHeight * image.width);
		document.body.style.setProperty('--piece-width', (pieceWidth) + 'px');
		document.body.style.setProperty('--piece-height', (pieceHeight) + 'px');
		document.body.style.setProperty('--nib-size', (nibSize) + 'px');
		document.body.style.setProperty('--image-width', (pieceWidth * puzzleWidth) + 'px');
		document.body.style.setProperty('--image-height', (pieceHeight * puzzleHeight) + 'px');
		let positions = [];
		for (let y = 0; y < puzzleHeight; y++) {
			for (let x = 0; x < puzzleWidth; x++) {
				positions.push([x, y, Math.random()]);
			}
		}
		//positions.sort((x, y) => x[2] - y[2]); // shuffle pieces
		for (let y = 0; y < puzzleHeight; y++) {
			for (let x = 0; x < puzzleWidth; x++) {
				let nibTypes = [null, null, null, null];
				let id = pieces.length;
				if (y > 0) nibTypes[0] = pieces[id - puzzleWidth].nibTypes[2].inverse();
				if (x < puzzleWidth - 1) nibTypes[1] = NibType.random(Math.floor(random() * 2) ? RIGHT_IN : RIGHT_OUT);
				if (y < puzzleHeight - 1) nibTypes[2] = NibType.random(Math.floor(random() * 2) ? BOTTOM_IN : BOTTOM_OUT);
				if (x > 0) nibTypes[3] = pieces[id - 1].nibTypes[1].inverse();
				pieces.push(new Piece(id, x * pieceWidth, y * pieceHeight, positions[id][0] * 80, positions[id][1] * 80, nibTypes));
			}
		}
	});
});
