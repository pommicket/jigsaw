window.addEventListener('load', function () {	
	const getById = (id) => document.getElementById(id);
	const customImageRadio = getById("custom-image");
	const customImageURL = getById("image-url");
	const pieceCountInput = getById("piece-count");
	const lastPieceCount = parseInt(localStorage.getItem('jigsaw.index.pieceCount'));
	if (isFinite(lastPieceCount) && lastPieceCount >= parseInt(pieceCountInput.min) && lastPieceCount <= parseInt(pieceCountInput.max)) {
		getById("piece-count").value = lastPieceCount;
	}
	function onImageTypeChange() {
		customImageURL.disabled = customImageRadio.checked ? '' : 'disabled';
	}
	onImageTypeChange();
	for (const radio of document.querySelectorAll('input[name=image]')) {
		radio.addEventListener("change", onImageTypeChange);
	}
	const hostForm = getById("host-form");
	hostForm.addEventListener("submit", function () {
		const formData = new FormData(hostForm);
		const pieceCount = formData.get('pieces');
		localStorage.setItem('jigsaw.index.pieceCount', pieceCount);
		const image = formData.get('image') === 'custom' ? formData.get('image-url') : formData.get('image');
		const search = new URLSearchParams();
		search.set('image', image);
		search.set('pieces', pieceCount);
		location.href = `game.html?${search}`;
	});
});
