/*
 * Copyright (c) 2014 Gabriele Cirulli
 *
 * SPDX-License-Identifier: MIT
 */

function KeyboardInputManager() {
    this.events = {};

    if (window.navigator.msPointerEnabled) {
        //Internet Explorer 10 style
        this.eventTouchstart = 'MSPointerDown';
        this.eventTouchmove = 'MSPointerMove';
        this.eventTouchend = 'MSPointerUp';
    } else {
        this.eventTouchstart = 'touchstart';
        this.eventTouchmove = 'touchmove';
        this.eventTouchend = 'touchend';
    }

    this.listen();
}

KeyboardInputManager.prototype.on = function (event, callback) {
    if (!this.events[event]) {
        this.events[event] = [];
    }
    this.events[event].push(callback);
};

KeyboardInputManager.prototype.emit = function (event, data) {
    var callbacks = this.events[event];
    if (callbacks) {
        callbacks.forEach(function (callback) {
            callback(data);
        });
    }
};

KeyboardInputManager.prototype.listen = function () {
    var self = this;

    var map = {
        38: 0, // Up
        39: 1, // Right
        40: 2, // Down
        37: 3, // Left
        75: 0, // Vim up
        76: 1, // Vim right
        74: 2, // Vim down
        72: 3, // Vim left
        87: 0, // W
        68: 1, // D
        83: 2, // S
        65: 3, // A
    };

    // Respond to direction keys
    document.addEventListener('keydown', function (event) {
        var modifiers = event.altKey || event.ctrlKey || event.metaKey || event.shiftKey;
        var mapped = map[event.which];

        if (!modifiers) {
            if (mapped !== undefined) {
                event.preventDefault();
                self.emit('move', mapped);
            }
        }

        // R key restarts the game
        if (!modifiers && event.which === 82) {
            self.restart.call(self, event);
        }
    });

    // Respond to button presses
    this.bindButtonPress('.retry-button', this.restart);
    this.bindButtonPress('.restart-button', this.restart);
    this.bindButtonPress('.keep-playing-button', this.keepPlaying);

    // Respond to swipe events, moving as soon as the finger travels far enough instead of
    // waiting until it lifts, short flicks still move when the finger lifts
    var touchStartClientX, touchStartClientY;
    var swiped = false;
    var gameContainer = document.getElementsByClassName('game-container')[0];

    function swipe(clientX, clientY, distance) {
        var dx = clientX - touchStartClientX;
        var absDx = Math.abs(dx);

        var dy = clientY - touchStartClientY;
        var absDy = Math.abs(dy);

        if (!swiped && Math.max(absDx, absDy) > distance) {
            swiped = true;
            // (right : left) : (down : up)
            self.emit('move', absDx > absDy ? (dx > 0 ? 1 : 3) : dy > 0 ? 2 : 0);
        }
    }

    gameContainer.addEventListener(this.eventTouchstart, function (event) {
        if ((!window.navigator.msPointerEnabled && event.touches.length > 1) || event.targetTouches.length > 1) {
            return; // Ignore if touching with more than 1 finger
        }

        if (window.navigator.msPointerEnabled) {
            touchStartClientX = event.pageX;
            touchStartClientY = event.pageY;
        } else {
            touchStartClientX = event.touches[0].clientX;
            touchStartClientY = event.touches[0].clientY;
        }
        swiped = false;

        event.preventDefault();
    });

    gameContainer.addEventListener(this.eventTouchmove, function (event) {
        event.preventDefault();
        if (window.navigator.msPointerEnabled) {
            swipe(event.pageX, event.pageY, 24);
        } else if (event.touches.length === 1) {
            swipe(event.touches[0].clientX, event.touches[0].clientY, 24);
        }
    });

    gameContainer.addEventListener(this.eventTouchend, function (event) {
        if ((!window.navigator.msPointerEnabled && event.touches.length > 0) || event.targetTouches.length > 0) {
            return; // Ignore if still touching with one or more fingers
        }

        if (window.navigator.msPointerEnabled) {
            swipe(event.pageX, event.pageY, 10);
        } else {
            swipe(event.changedTouches[0].clientX, event.changedTouches[0].clientY, 10);
        }
        // End the gesture, pointer move events also fire while hovering
        swiped = true;
    });
};

KeyboardInputManager.prototype.restart = function (event) {
    event.preventDefault();
    this.emit('restart');
};

KeyboardInputManager.prototype.keepPlaying = function (event) {
    event.preventDefault();
    this.emit('keepPlaying');
};

KeyboardInputManager.prototype.bindButtonPress = function (selector, fn) {
    var button = document.querySelector(selector);
    button.addEventListener('click', fn.bind(this));
    button.addEventListener(this.eventTouchend, fn.bind(this));
};
