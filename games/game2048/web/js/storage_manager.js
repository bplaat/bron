/*
 * Copyright (c) 2014 Gabriele Cirulli
 * Copyright (c) 2025 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

// Keeps the state loaded from the native side and sends every change back over IPC
function StorageManager(state) {
    this.bestScore = state.bestScore;
    this.gameState = state.gameState;
}

// Best score getters/setters
StorageManager.prototype.getBestScore = function () {
    return this.bestScore;
};

StorageManager.prototype.setBestScore = function (score) {
    this.bestScore = score;
    this.save();
};

// Game state getters/setters and clearing
StorageManager.prototype.getGameState = function () {
    return this.gameState;
};

StorageManager.prototype.setGameState = function (gameState) {
    this.gameState = gameState;
    this.save();
};

StorageManager.prototype.clearGameState = function () {
    this.gameState = null;
    this.save();
};

StorageManager.prototype.save = function () {
    window.ipc.postMessage(JSON.stringify({ type: 'state', bestScore: this.bestScore, gameState: this.gameState }));
};
