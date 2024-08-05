window.addEventListener('load', function () {
	const getById = (id) => document.getElementById(id);
	const playArea = getById("play-area");
	const scale = 0.2;
	let pieceZIndexCounter = 1;
	let draggingPiece = null;
	let pieceSize = 50;
	let nibSize = 10;
	const draggingPieceLastPos = Object.preventExtensions({x: null, y: null});
	class NibType {
		dx11;
		dy11;
		dx12;
		dy12;
		dx21;
		dy21;
		dx22;
		dy22;
		randomize() {
			const bendiness = 0.5;
			this.dx11 = Math.floor((Math.random() *  2 - 1)  * nibSize * bendiness);
			this.dy11 = nibSize / 2 + Math.floor((Math.random() * 2 - 1) * bendiness);
			this.dx12 = Math.floor((Math.random() *  2 - 1) * nibSize * bendiness);
			// this ensures base of nib is flat
			this.dy12 = nibSize;
			this.dx22 = nibSize / 2 + Math.floor((Math.random() *  2 - 1) * nibSize * bendiness);
			this.dy22 = -nibSize / 2 + Math.floor((Math.random() * 2 - 1) * nibSize * bendiness);
			return this;
		}
		path() {
			return `c${this.dx11} ${this.dy11} ${this.dx12} ${this.dy12} ${nibSize / 2} ${nibSize}`
				+ ` s${this.dx22} ${this.dy22} ${nibSize / 2} ${-nibSize}`;
		}
	}
	class Piece {
		id;
		u;
		v;
		x;
		y;
		element;
		constructor(id, u, v, x, y) {
			this.id = id;
			this.x = x;
			this.y = y;
			this.u = u;
			this.v = v;
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
			let clipPath = [`path("M${nibSize} ${nibSize}`];
			clipPath.push(`l${shoulderWidth} 0`);
			clipPath.push(new NibType().randomize().path());
			clipPath.push(`L${pieceSize + nibSize} ${nibSize}`);
			clipPath.push(`L${pieceSize + nibSize} ${pieceSize + nibSize}`);
			clipPath.push(`L${nibSize} ${pieceSize + nibSize}`);
			clipPath.push(`L${nibSize} ${nibSize}`);
			this.element.style.clipPath = clipPath.join(' ');
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
			draggingPiece.element.style.removeProperty('cursor');
			draggingPiece = null;
		}
	});
	window.addEventListener('mousemove', function(e) {
		if (draggingPiece) {
			draggingPiece.x += e.clientX - draggingPieceLastPos.x;
			draggingPiece.y += e.clientY - draggingPieceLastPos.y;
			draggingPiece.updatePosition();
			draggingPieceLastPos.x = e.clientX;
			draggingPieceLastPos.y = e.clientY;
		}
	});
	const pieces = [];
	for (let y = 0; y < 12; y++) {
		for (let x = 0; x < 19; x++) {
			pieces.push(new Piece(pieces.length, x * pieceSize, y * pieceSize, x * 60, y * 60));
		}
	}
});
