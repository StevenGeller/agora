(function() {
    "use strict";

    var flowEl = document.getElementById('flow');
    var flowHeader = document.getElementById('flow-header');
    var walletBar = document.getElementById('wallet-bar');
    var themeBtn = document.getElementById('theme-toggle');
    var buttons = document.querySelectorAll('.card button');
    var toggleBtns = document.querySelectorAll('.toggle-btn');
    var running = false;
    var selectedProtocol = 'x402-testnet';

    var headerLabels = {
        'x402-testnet': 'x402 v2 Handshake (Base Sepolia)',
        'x402-mainnet': 'x402 v2 Handshake (Base Mainnet)',
        'mpp': 'MPP Handshake (Tempo)'
    };

    // --- Theme toggle ---

    function getTheme() {
        return document.documentElement.getAttribute('data-theme') || 'light';
    }

    function updateThemeButton() {
        themeBtn.textContent = getTheme() === 'dark' ? 'light' : 'dark';
    }

    updateThemeButton();

    themeBtn.addEventListener('click', function() {
        var next = getTheme() === 'dark' ? 'light' : 'dark';
        document.documentElement.setAttribute('data-theme', next);
        localStorage.setItem('agora-theme', next);
        updateThemeButton();
    });

    // --- Wallet balance ---

    function fetchBalance() {
        walletBar.textContent = '';
        fetch('/demo/balance', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ protocol: selectedProtocol })
        })
        .then(function(r) { return r.json(); })
        .then(function(data) {
            if (data.error) {
                walletBar.textContent = '';
                return;
            }
            var short = data.wallet.slice(0, 6) + '...' + data.wallet.slice(-4);
            walletBar.innerHTML =
                '<span class="bal">' + esc(data.balance) + ' ' + esc(data.token) + '</span> ' +
                '<span class="addr">' + esc(short) + ' · ' + esc(data.chain) + '</span>';
        })
        .catch(function() { walletBar.textContent = ''; });
    }

    fetchBalance();

    // --- Protocol toggle ---

    for (var t = 0; t < toggleBtns.length; t++) {
        toggleBtns[t].addEventListener('click', function() {
            if (running) return;
            for (var j = 0; j < toggleBtns.length; j++) toggleBtns[j].classList.remove('active');
            this.classList.add('active');
            selectedProtocol = this.getAttribute('data-proto');
            flowHeader.textContent = headerLabels[selectedProtocol] || 'Handshake';
            fetchBalance();
        });
    }

    // --- Purchase buttons ---

    for (var b = 0; b < buttons.length; b++) {
        buttons[b].addEventListener('click', function() {
            var endpoint = this.getAttribute('data-endpoint');
            if (endpoint) purchase(endpoint);
        });
    }

    function setButtons(enabled) {
        for (var i = 0; i < buttons.length; i++) {
            buttons[i].disabled = !enabled;
        }
    }

    function purchase(endpoint) {
        if (running) return;
        running = true;
        setButtons(false);

        flowEl.className = 'flow';
        flowEl.innerHTML = '';
        flowHeader.textContent = headerLabels[selectedProtocol] || 'Handshake';

        fetch('/demo/purchase', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ endpoint: endpoint, protocol: selectedProtocol })
        })
        .then(function(r) { return r.json(); })
        .then(function(data) {
            if (data.error && !data.steps) {
                addStep('error', 'Error', data.error, null, 0);
                running = false;
                setButtons(true);
                return;
            }

            var steps = data.steps || [];
            var delay = 0;

            for (var i = 0; i < steps.length; i++) {
                (function(step, d) {
                    setTimeout(function() {
                        renderStep(step, data);
                    }, d);
                })(steps[i], delay);
                delay += 350;
            }

            setTimeout(function() {
                if (data.elapsed_ms !== undefined) {
                    var el = document.createElement('div');
                    el.className = 'flow-elapsed';
                    el.textContent = 'completed in ' + data.elapsed_ms + 'ms';
                    flowEl.appendChild(el);
                }

                if (data.result) {
                    renderResult(data.result, endpoint);
                }

                running = false;
                setButtons(true);
                fetchBalance();
            }, delay);
        })
        .catch(function(err) {
            addStep('error', 'Error', err.toString(), null, 0);
            running = false;
            setButtons(true);
        });
    }

    // --- Rendering ---

    function renderStep(step, data) {
        var labelClass = 'request';
        var label = step.name;

        if (step.name === '402') { labelClass = 's402'; label = '402 Payment Required'; }
        else if (step.name === 'sign') { labelClass = 'sign'; label = 'Sign Payment'; }
        else if (step.name === 'transfer') { labelClass = 'transfer'; label = 'Transfer'; }
        else if (step.name === 'settled') { labelClass = 'settled'; label = 'Settled'; }
        else if (step.name === 'retry') { labelClass = 'retry'; label = 'Retry with Payment'; }
        else if (step.name === '200') { labelClass = 's200'; label = '200 OK'; }
        else if (step.name === 'error') { labelClass = 'error'; label = 'Error'; }
        else if (step.name === 'request') { labelClass = 'request'; label = 'Initial Request'; }

        var div = document.createElement('div');
        div.className = 'step';

        var html = '<span class="step-label ' + labelClass + '">' + esc(label) + '</span>';

        if (step.detail) {
            html += '<div class="step-detail">' + esc(step.detail) + '</div>';
        }
        if (step.wallet) {
            html += '<div class="step-detail">wallet: ' + esc(step.wallet) + '</div>';
        }

        if (step.headers) {
            var payload = JSON.stringify(step.headers, null, 2);
            if (payload.length > 2000) {
                payload = payload.substring(0, 2000) + '\n... (truncated)';
            }
            html += '<div class="step-payload">' + esc(payload) + '</div>';
        }

        div.innerHTML = html;
        flowEl.appendChild(div);
    }

    function renderResult(result, endpoint) {
        var div = document.createElement('div');
        div.className = 'step';

        if (endpoint === 'torus' && result.svg) {
            var container = document.createElement('div');
            container.className = 'step-result';
            container.style.textAlign = 'center';
            container.appendChild(sanitizeSvg(result.svg));
            div.appendChild(container);
        } else if (result.haiku) {
            div.innerHTML = '<div class="step-result">' + esc(result.haiku) + '</div>';
        } else if (result.quote) {
            div.innerHTML = '<div class="step-result">"' + esc(result.quote) + '"<br><br>-- ' + esc(result.author) + '</div>';
        } else if (result.fact) {
            div.innerHTML = '<div class="step-result">' + esc(result.fact) + '</div>';
        } else {
            div.innerHTML = '<div class="step-result">' + esc(JSON.stringify(result, null, 2)) + '</div>';
        }

        flowEl.appendChild(div);
    }

    // --- SVG sanitization ---

    var ALLOWED_SVG_ELEMENTS = [
        'svg', 'g', 'path', 'circle', 'ellipse', 'line', 'polyline', 'polygon',
        'rect', 'text', 'tspan', 'defs', 'clippath', 'use', 'symbol',
        'lineargradient', 'radialgradient', 'stop', 'mask', 'pattern', 'title', 'desc'
    ];

    function sanitizeSvg(svgString) {
        var parser = new DOMParser();
        var doc = parser.parseFromString(svgString, 'image/svg+xml');
        var svg = doc.documentElement;

        if (svg.nodeName === 'parsererror' || !svg.nodeName || svg.nodeName !== 'svg') {
            var fallback = document.createElement('span');
            fallback.textContent = 'invalid svg';
            return fallback;
        }

        cleanNode(svg);

        svg.style.maxWidth = '200px';
        svg.style.height = 'auto';
        svg.style.display = 'inline-block';

        return document.importNode(svg, true);
    }

    function cleanNode(node) {
        // Remove event handler attributes
        if (node.attributes) {
            for (var i = node.attributes.length - 1; i >= 0; i--) {
                var name = node.attributes[i].name.toLowerCase();
                if (name.startsWith('on') || name === 'href' && node.getAttribute('href').indexOf('javascript') === 0) {
                    node.removeAttribute(node.attributes[i].name);
                }
            }
        }

        // Walk children and remove disallowed elements
        var children = node.childNodes;
        for (var j = children.length - 1; j >= 0; j--) {
            var child = children[j];
            if (child.nodeType === 1) {
                var tag = child.nodeName.toLowerCase();
                if (ALLOWED_SVG_ELEMENTS.indexOf(tag) === -1) {
                    node.removeChild(child);
                } else {
                    cleanNode(child);
                }
            }
        }
    }

    // --- Helpers ---

    function addStep(cls, label, detail, payload, delay) {
        setTimeout(function() {
            var div = document.createElement('div');
            div.className = 'step';
            var html = '<span class="step-label ' + cls + '">' + esc(label) + '</span>';
            if (detail) html += '<div class="step-detail">' + esc(detail) + '</div>';
            if (payload) html += '<div class="step-payload">' + esc(payload) + '</div>';
            div.innerHTML = html;
            flowEl.appendChild(div);
        }, delay);
    }

    function esc(s) {
        if (!s) return '';
        var d = document.createElement('div');
        d.textContent = s;
        return d.innerHTML;
    }

})();
