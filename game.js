window.addEventListener('load', function () {
	const getById = (id) => document.getElementById(id);
	const playArea = getById("play-area");
	const scale = 0.2;
	let draggingPiece = null;
	class Piece {
		constructor(id, u, v, x, y) {
			this.id = id;
			this.u = u;
			this.v = v;
			const element = this.element = document.createElement('div');
			element.classList.add('piece');
			element.style.backgroundPositionX = (-u) + 'px';
			element.style.backgroundPositionY = (-v) + 'px';
			element.style.left = x + 'px';
			element.style.top = y + 'px';
			element.addEventListener('click', function() {
				draggingPiece = this;
			});
			playArea.appendChild(element);
		}
	}
	window.addEventListener('mouseup', function() {
		draggingPiece = null;
	});
	window.addEventListener('mousemove', function(e) {
		e.movementX;
		
	});
	const pieces = [];
	for (let y = 0; y < 12; y++) {
		for (let x = 0; x < 19; x++) {
			pieces.push(new Piece(pieces.length, x * 40, y * 40, x * 50, y * 50));
		}
	}
});
