'use strict';
window.addEventListener('load', function () {
	const getById = (id) => document.getElementById(id);
	const playArea = getById("play-area");
	const connectRadius = 5;
	let pieceZIndexCounter = 1;
	let draggingPiece = null;
	let nibSize = 12;
	let pieceSize = 70 - 2 * nibSize;
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
			let shoulderWidth = (pieceSize - nibSize) / 2;
			const debugCurves = false;//display bezier control points for debugging
			if (debugCurves)
				playArea.appendChild(element);
			const debugPoint = (x, y, color) => {
				if (debugCurves)
					debugAddPoint(this.element, x, y, color, this.id);
			};
			const debugPath = (path, x0, y0) => {
				if (!debugCurves) return;
				path = path.replace(/[cs]/g, '').split(' ').map((x) => parseFloat(x));
				console.assert(path.length === 10);
				debugPoint(x0, y0, 'green');
				debugPoint(x0 + path[0], y0 + path[1], 'blue');
				debugPoint(x0 + path[2], y0 + path[3], 'cyan');
				debugPoint(x0 + path[4], y0 + path[5], 'green');
				// reflected point
				debugPoint(x0 + 2 * path[4] - path[2], y0 + 2 * path[5] - path[3], 'red');
				debugPoint(x0 + path[4] + path[6], y0 + path[5] + path[7], 'magenta');
				debugPoint(x0 + path[4] + path[8], y0 + path[5] + path[9], 'green');
			};
			this.nibTypes = nibTypes;
			let clipPath = [`path("M${nibSize} ${nibSize}`];
			clipPath.push(`l${shoulderWidth} 0`);
			if (nibTypes[0]) {
				debugPath(nibTypes[0].path(), nibSize + shoulderWidth, nibSize);
				clipPath.push(nibTypes[0].path());
			}
			clipPath.push(`L${pieceSize + nibSize} ${nibSize}`);
			clipPath.push(`l0 ${shoulderWidth}`);
			if (nibTypes[1]) {
				debugPath(nibTypes[1].path(), pieceSize + nibSize, nibSize + shoulderWidth);
				clipPath.push(nibTypes[1].path());
			}
			clipPath.push(`L${pieceSize + nibSize} ${pieceSize + nibSize}`);
			clipPath.push(`l-${shoulderWidth} 0`);
			if (nibTypes[2]) {
				debugPath(nibTypes[2].path(), pieceSize + nibSize - shoulderWidth, pieceSize + nibSize);
				clipPath.push(nibTypes[2].path());
			}
			clipPath.push(`L${nibSize} ${pieceSize + nibSize}`);
			clipPath.push(`l0 -${shoulderWidth}`);
			if (nibTypes[3]) clipPath.push(nibTypes[3].path());
			clipPath.push(`L${nibSize} ${nibSize}`);
			this.element.style.clipPath = clipPath.join(' ');
			if (!debugCurves)
				playArea.appendChild(element);
		}
		updateUV() {
			this.element.style.backgroundPositionX = (-this.u) + 'px';
			this.element.style.backgroundPositionY = (-this.v) + 'px';
		}
		updatePosition() {
			this.element.style.left = this.x + 'px';
			this.element.style.top = this.y + 'px';
		}
	}
	window.addEventListener('mouseup', function() {
		if (draggingPiece) {
			for (const piece of draggingPiece.connectedComponent) {
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
						connectPieces(piece, neighbour);
					}
				}
			}
			draggingPiece.element.style.removeProperty('cursor');
			draggingPiece = null;
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
	let puzzleWidth = 19;
	let puzzleHeight = 12;
	for (let y = 0; y < puzzleHeight; y++) {
		for (let x = 0; x < puzzleWidth; x++) {
			let nibTypes = [null, null, null, null];
			let id = pieces.length;
			if (y > 0) nibTypes[0] = pieces[id - puzzleWidth].nibTypes[2].inverse();
			if (x < puzzleWidth - 1) nibTypes[1] = NibType.random(Math.floor(random() * 2) ? RIGHT_IN : RIGHT_OUT);
			if (y < puzzleHeight - 1) nibTypes[2] = NibType.random(Math.floor(random() * 2) ? BOTTOM_IN : BOTTOM_OUT);
			if (x > 0) nibTypes[3] = pieces[id - 1].nibTypes[1].inverse();
			pieces.push(new Piece(id, x * pieceSize, y * pieceSize, x * 70, y * 70, nibTypes));
		}
	}
});
