// The connection is peer to peer or nothing. ecdh.chat makes its connections with
// `new RTCPeerConnection(config)`; this wraps that before the first one is made:
//
//   - TURN servers are taken out of the configuration, so this side never uses a relay;
//   - the other side's relay candidates are dropped, whether trickled or in a description;
//   - once connected, the route the connection actually took is read from its stats, and a relayed
//     one (on either end) is reported, so the chat can be ended.

export function isRelayCandidate(line) {
  return /\styp\s+relay(\s|$)/i.test(String(line));
}

export function directOnly(config) {
  const c = Object.assign({}, config || {});
  c.iceServers = ((config && config.iceServers) || []).map((server) => {
    const urls = [].concat((server && server.urls) || []).filter((u) => !/^turns?:/i.test(String(u)));
    return urls.length ? { urls } : null;
  }).filter(Boolean);
  c.iceTransportPolicy = 'all';
  return c;
}

// onRelay(): a connection ended up relayed. Returns { relaySeen() }: whether a relay candidate of
// theirs was dropped, which explains a connection that then failed.
export function installDirectOnly(win, onRelay) {
  let seen = false;
  const Native = win.RTCPeerConnection;
  if (typeof Native !== 'function') return { relaySeen: () => seen };

  const proto = Native.prototype;
  const addIceCandidate = proto.addIceCandidate;
  proto.addIceCandidate = function (candidate) {
    const line = candidate && (candidate.candidate || (typeof candidate === 'string' ? candidate : ''));
    if (line && isRelayCandidate(line)) { seen = true; return Promise.resolve(); }
    return addIceCandidate.apply(this, arguments);
  };
  const setRemoteDescription = proto.setRemoteDescription;
  proto.setRemoteDescription = function (description) {
    if (description && typeof description.sdp === 'string') {
      const sdp = description.sdp.split('\r\n').filter((line) => {
        if (!line.startsWith('a=candidate:') || !isRelayCandidate(line)) return true;
        seen = true;
        return false;
      }).join('\r\n');
      description = { type: description.type, sdp };
    }
    return setRemoteDescription.call(this, description);
  };

  function checkRoute(pc) {
    if (!pc.getStats || pc.connectionState === 'closed') return;
    pc.getStats().then((report) => {
      let pair = null;
      report.forEach((s) => { if (s.type === 'transport' && s.selectedCandidatePairId) pair = report.get(s.selectedCandidatePairId) || pair; });
      if (!pair) report.forEach((s) => { if (!pair && s.type === 'candidate-pair' && s.nominated && s.state === 'succeeded') pair = s; });
      if (!pair) return;
      const local = report.get(pair.localCandidateId);
      const remote = report.get(pair.remoteCandidateId);
      if ((local && local.candidateType === 'relay') || (remote && remote.candidateType === 'relay')) onRelay(pc);
    }).catch(() => {});
  }

  const Direct = function RTCPeerConnection(config) {
    const pc = new Native(directOnly(config));
    pc.addEventListener('connectionstatechange', () => {
      if (pc.connectionState === 'connected') checkRoute(pc);
    });
    // A route can change after it connects (an ICE restart, a network change).
    const timer = win.setInterval(() => {
      if (pc.connectionState === 'closed') win.clearInterval(timer);
      else if (pc.connectionState === 'connected') checkRoute(pc);
    }, 5000);
    return pc;
  };
  Direct.prototype = proto;
  if (Native.generateCertificate) Direct.generateCertificate = Native.generateCertificate.bind(Native);
  win.RTCPeerConnection = Direct;
  return { relaySeen: () => seen };
}
