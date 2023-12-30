import init, { Emulator } from "./pkg/gb.js";

class Display {
  constructor(canvas, currentGame, canvasScale) {
    this.currentGame = currentGame;
    this.ctx = canvas.getContext("2d");
    this.currentKeypress = -1;
    this.canvasScale = canvasScale;
  }

  changeGame(currentGame) {
    if (this.currentGame) {
      cancelAnimationFrame(currentGame);
    }
    this.currentGame = currentGame;
  }

  changeCanvasDimensions(width, height) {
    this.ctx.canvas.width = width * this.canvasScale;
    this.ctx.canvas.height = height * this.canvasScale;
  }
}

class Gameboy extends Display {
  constructor(canvas, currentGame, canvasScale) {
    super(canvas, currentGame, canvasScale);
    super.changeCanvasDimensions(160, 144);
    this.colorPallete = ["#FFFFFF", "#AAAAAA", "#555555", "#000000"];
  }

  run(cartridge) {
    fetch("binaries/DMG_ROM.bin")
      .then((res) => res.arrayBuffer())
      .then((boot) => {
        const emulator = Emulator.new();

        emulator.load_bootrom(new Uint8Array(boot));
        emulator.load_catridge(new Uint8Array(cartridge));

        let ctx = this.ctx;
        let colorPallete = this.colorPallete;
        let canvasScale = this.canvasScale;
        let currentGame;

        function animate() {
          let display = emulator.render();
          for (let row = 0; row < 144; row++) {
            for (let col = 0; col < 160; col++) {
              ctx.fillStyle = colorPallete[display[row * 160 + col]];
              ctx.fillRect(col * canvasScale, row * canvasScale, canvasScale, canvasScale);
            }
          }
          currentGame = requestAnimationFrame(animate);
        }
        animate();

        this.changeGame(currentGame);
      });
  }
}

init().then(() => {
  const canvas = document.getElementById("emulator");

  const gameboy = new Gameboy(canvas, null, 3);

  const romSelect = document.getElementById("rom-select-gb");
  romSelect.addEventListener("click", function (e) {
    fetch(`roms/gb/${e.target.value}`)
      .then((res) => res.arrayBuffer())
      .then((buffer) => {
        gameboy.run(buffer);
      });
  });

  canvas.addEventListener("click", function () {});
});
