// Runs in every frame of the main window (`create_main_window`), and does something only in the
// frames under the app's page: an embedded player's. Once a second, a frame that holds a video with
// a length posts where it is to the app's page, which can't read a cross-origin frame itself but
// can be posted to by any of them. `EmbedPlayer` turns it into the Discord card's progress bar and
// pause, as Android's `AniliProgress.onTick` does. Ads play videos too; the page sorts them out.
(function () {
  if (window.top === window || window.__aniliEmbedProgress) return
  window.__aniliEmbedProgress = true
  setInterval(function () {
    try {
      var video = document.querySelector("video")
      if (!video || !isFinite(video.duration) || video.duration <= 0 || !(video.currentTime >= 0)) return
      window.top.postMessage(
        { aniliEmbedProgress: { position: video.currentTime, duration: video.duration, paused: video.paused } },
        "*",
      )
    } catch (e) {
      /* a frame on its way out */
    }
  }, 1000)
})()
