(function() {
    "use strict";

    var flowEl = document.getElementById('flow');
    var flowHeader = document.getElementById('flow-header');
    var walletBar = document.getElementById('wallet-bar');
    var balanceWarn = document.getElementById('balance-warn');
    var explainerEl = document.getElementById('proto-explainer');
    var themeBtn = document.getElementById('theme-toggle');
    var priceEls = document.querySelectorAll('[data-price]');
    var buttons = document.querySelectorAll('.card button');
    var toggleBtns = document.querySelectorAll('.toggle-btn');
    var running = false;
    var currentExplorer = '';

    // --- URL routing ---

    var validProtos = ['x402-testnet', 'x402-mainnet', 'mpp-testnet', 'mpp-mainnet'];

    function getProtoFromURL() {
        var params = new URLSearchParams(window.location.search);
        var p = params.get('protocol');
        return (p && validProtos.indexOf(p) !== -1) ? p : 'x402-testnet';
    }

    function setProtoURL(proto) {
        var url = new URL(window.location);
        url.searchParams.set('protocol', proto);
        history.replaceState(null, '', url);
    }

    var selectedProtocol = getProtoFromURL();

    // Set initial active toggle from URL
    (function() {
        for (var i = 0; i < toggleBtns.length; i++) {
            toggleBtns[i].classList.toggle('active', toggleBtns[i].getAttribute('data-proto') === selectedProtocol);
        }
    })();

    // --- Number formatting ---

    function formatBalance(raw) {
        var n = parseFloat(raw);
        if (isNaN(n)) return raw;
        return n.toLocaleString('en-US', { minimumFractionDigits: 2, maximumFractionDigits: 2 });
    }

    // --- Protocol metadata ---

    var protoInfo = {
        'x402-testnet': {
            header: 'x402 v2 Handshake (Base Sepolia)',
            token: 'USDC',
            explainer: '<strong>x402</strong> is Coinbase\'s HTTP payment protocol (x402.org). The server returns a 402 with payment instructions. The client signs an EIP-712 typed-data message authorizing a USDC transfer via ERC-3009. A facilitator settles the payment on Base, and the server delivers content once the facilitator confirms.'
        },
        'x402-mainnet': {
            header: 'x402 v2 Handshake (Base Mainnet)',
            token: 'USDC',
            explainer: '<strong>x402 mainnet</strong> uses real USDC on Base L2. Same protocol as testnet, but settled with real money. The demo wallet currently has no mainnet USDC.'
        },
        'mpp-testnet': {
            header: 'MPP Handshake (Tempo Moderato)',
            token: 'pathUSD',
            explainer: '<strong>MPP</strong> is Stripe\'s Machine Payments Protocol (IETF draft-ryan-httpauth-payment). The server returns a 402 with a WWW-Authenticate: Payment header containing an HMAC-bound challenge. The client sends a real on-chain pathUSD transfer on the Tempo chain, then retries with an Authorization: Payment credential containing the tx hash as proof. The server verifies the receipt directly on-chain, no facilitator needed.'
        },
        'mpp-mainnet': {
            header: 'MPP Handshake (Tempo Mainnet)',
            token: 'pathUSD',
            explainer: '<strong>MPP mainnet</strong> uses real pathUSD on Tempo\'s production chain. Same protocol as testnet, but settled with real tokens. The demo wallet currently has no mainnet pathUSD.'
        }
    };

    // Human-readable descriptions for each step
    var stepMeta = {
        'request':  { label: 'Initial Request',      cls: 'request',  desc: 'Client sends a normal GET request without any payment credentials' },
        '402':      { label: '402 Payment Required',  cls: 's402',     desc: '' },
        'sign':     { label: 'Sign Payment',          cls: 'sign',     desc: '' },
        'transfer': { label: 'On-Chain Transfer',     cls: 'transfer', desc: 'Client broadcasts a real token transfer to the blockchain' },
        'settled':  { label: 'Confirmed On-Chain',    cls: 'settled',  desc: 'Transfer is included in a block and confirmed by the network' },
        'retry':    { label: 'Retry with Payment',    cls: 'retry',    desc: 'Client replays the original request, now with proof of payment attached' },
        '200':      { label: '200 OK',                cls: 's200',     desc: 'Server verifies the payment and delivers the purchased content' },
        'error':    { label: 'Error',                 cls: 'error',    desc: '' }
    };

    // 402 step description depends on protocol
    function get402Desc() {
        if (selectedProtocol.startsWith('mpp')) {
            return 'Server returns WWW-Authenticate: Payment with an HMAC-bound challenge (standard HTTP auth)';
        }
        return 'Server returns a Payment-Required header with payment instructions (x402 proprietary header)';
    }

    // Sign step description depends on protocol
    function getSignDesc() {
        if (selectedProtocol.startsWith('mpp')) {
            return 'Client builds an Authorization: Payment credential containing the challenge and tx hash';
        }
        return 'EIP-712 off-chain signature. The facilitator settles on-chain.';
    }

    // --- Theme toggle ---

    function getTheme() { return document.documentElement.getAttribute('data-theme') || 'light'; }
    function updateThemeButton() { themeBtn.textContent = getTheme() === 'dark' ? 'light' : 'dark'; }
    updateThemeButton();

    themeBtn.addEventListener('click', function() {
        var next = getTheme() === 'dark' ? 'light' : 'dark';
        document.documentElement.setAttribute('data-theme', next);
        localStorage.setItem('agora-theme', next);
        updateThemeButton();
    });

    // --- Protocol UI updates ---

    function updateProtocolUI() {
        var info = protoInfo[selectedProtocol] || {};
        flowHeader.textContent = info.header || 'Handshake';
        explainerEl.innerHTML = info.explainer || '';

        // Update card prices
        var token = info.token || 'USDC';
        for (var i = 0; i < priceEls.length; i++) {
            priceEls[i].textContent = '0.001 ' + token;
        }
    }

    updateProtocolUI();

    // --- Wallet balance ---

    function fetchBalance() {
        walletBar.textContent = '';
        balanceWarn.textContent = '';
        fetch('/demo/balance', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ protocol: selectedProtocol })
        })
        .then(function(r) { return r.json(); })
        .then(function(data) {
            if (data.error) { walletBar.textContent = ''; return; }
            currentExplorer = data.explorer || '';
            var addrLink = currentExplorer
                ? '<a href="' + esc(currentExplorer) + '/address/' + esc(data.wallet) + '" target="_blank" rel="noopener" class="explorer-link">' + esc(data.wallet) + '</a>'
                : esc(data.wallet);
            walletBar.innerHTML =
                '<span class="bal">' + esc(formatBalance(data.balance)) + ' ' + esc(data.token) + '</span> ' +
                '<span class="addr">' + addrLink + ' · ' + esc(data.chain) + '</span>';

            // Zero balance warning
            if (data.balance === '0.000000') {
                balanceWarn.textContent = 'wallet has no ' + data.token + ' on this network — purchases will fail';
            }
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
            setProtoURL(selectedProtocol);
            updateProtocolUI();
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
        for (var i = 0; i < buttons.length; i++) buttons[i].disabled = !enabled;
    }

    function purchase(endpoint) {
        if (running) return;
        running = true;
        setButtons(false);

        flowEl.className = 'flow';
        flowEl.innerHTML = '';

        // Show loading indicator in header
        flowHeader.innerHTML = esc(protoInfo[selectedProtocol].header || 'Handshake') +
            ' <span class="loading-dot"></span>';

        fetch('/demo/purchase', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ endpoint: endpoint, protocol: selectedProtocol })
        })
        .then(function(r) { return r.json(); })
        .then(function(data) {
            // Remove loading dot
            flowHeader.textContent = protoInfo[selectedProtocol].header || 'Handshake';

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
                    setTimeout(function() { renderStep(step); }, d);
                })(steps[i], delay);
                delay += 400;
            }

            setTimeout(function() {
                if (data.elapsed_ms !== undefined) {
                    var el = document.createElement('div');
                    el.className = 'flow-elapsed';
                    el.textContent = 'total: ' + data.elapsed_ms + 'ms';
                    flowEl.appendChild(el);
                }
                if (data.result) renderResult(data.result, endpoint);
                running = false;
                setButtons(true);
                fetchBalance();
            }, delay);
        })
        .catch(function(err) {
            flowHeader.textContent = protoInfo[selectedProtocol].header || 'Handshake';
            addStep('error', 'Error', err.toString(), null, 0);
            running = false;
            setButtons(true);
        });
    }

    // --- Rendering ---

    function txLink(hash) {
        if (!hash || !currentExplorer) return esc(hash);
        return '<a href="' + esc(currentExplorer) + '/tx/' + esc(hash) + '" target="_blank" rel="noopener" class="explorer-link">' + esc(hash) + '</a>';
    }

    function renderStep(step) {
        var meta = stepMeta[step.name] || { label: step.name, cls: 'request', desc: '' };
        var desc = step.name === 'sign' ? getSignDesc() : step.name === '402' ? get402Desc() : meta.desc;

        var div = document.createElement('div');
        div.className = 'step';

        var html = '<span class="step-label ' + meta.cls + '">' + esc(meta.label) + '</span>';

        if (desc) {
            html += '<div class="step-desc">' + esc(desc) + '</div>';
        }
        if (step.detail) {
            html += '<div class="step-detail">' + esc(step.detail) + '</div>';
        }
        if (step.wallet) {
            var walletLabel = (step.name === 'sign' && selectedProtocol.startsWith('x402')) ? 'facilitator' : 'wallet';
            html += '<div class="step-detail">' + walletLabel + ': ' + walletLink(step.wallet) + '</div>';
        }
        if (step.tx_hash) {
            html += '<div class="step-detail">tx: ' + txLink(step.tx_hash) + '</div>';
        }
        if (step.headers) {
            var payload = JSON.stringify(step.headers, null, 2);
            if (payload.length > 2000) payload = payload.substring(0, 2000) + '\n... (truncated)';
            html += '<div class="step-payload">' + esc(payload) + '</div>';
        }

        div.innerHTML = html;
        flowEl.appendChild(div);
    }

    function walletLink(addr) {
        if (currentExplorer) {
            return '<a href="' + esc(currentExplorer) + '/address/' + esc(addr) + '" target="_blank" rel="noopener" class="explorer-link">' + esc(addr) + '</a>';
        }
        return esc(addr);
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

    var ALLOWED_SVG = ['svg','g','path','circle','ellipse','line','polyline','polygon','rect','text','tspan','defs','clippath','use','symbol','lineargradient','radialgradient','stop','mask','pattern','title','desc'];

    function sanitizeSvg(str) {
        var doc = new DOMParser().parseFromString(str, 'image/svg+xml');
        var svg = doc.documentElement;
        if (!svg || svg.nodeName !== 'svg') {
            var f = document.createElement('span'); f.textContent = 'invalid svg'; return f;
        }
        cleanNode(svg);
        svg.style.maxWidth = '200px'; svg.style.height = 'auto'; svg.style.display = 'inline-block';
        return document.importNode(svg, true);
    }

    function cleanNode(n) {
        if (n.attributes) {
            for (var i = n.attributes.length - 1; i >= 0; i--) {
                var nm = n.attributes[i].name.toLowerCase();
                if (nm.startsWith('on') || (nm === 'href' && String(n.getAttribute('href')).indexOf('javascript') === 0)) {
                    n.removeAttribute(n.attributes[i].name);
                }
            }
        }
        for (var j = n.childNodes.length - 1; j >= 0; j--) {
            var c = n.childNodes[j];
            if (c.nodeType === 1) {
                if (ALLOWED_SVG.indexOf(c.nodeName.toLowerCase()) === -1) n.removeChild(c);
                else cleanNode(c);
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
