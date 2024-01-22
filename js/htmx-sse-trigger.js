// WIP
// Custom HTMX Extension for sse.js
// Supports SSE post requests and out of bound swapping from event stream

import { SSE } from 'sse.js'

var VERBS = ['get', 'post', 'put', 'delete', 'patch']
var VERB_SELECTOR = VERBS.map(function (verb) {
  return '[hx-sse-' + verb + '], [data-hx-sse-' + verb + ']'
}).join(', ')

htmx.defineExtension('trigger-sse', {
  /**
   * Init saves the provided reference to the internal HTMX API.
   *
   * @param {import("../htmx").HtmxInternalApi} api
   * @returns void
   */
  init: function (apiRef) {
    // store a reference to the internal API.
    api = apiRef
    // set a function in the public API for creating new EventSource objects
    htmx.createEventSource = createEventSource
  },

  /**
   * onEvent handles all events passed to this extension.
   *
   * @param {string} name
   * @param {Event} evt
   * @returns void
   */
  onEvent: function (name, evt) {
    console.log('HTMX >>>', name, evt)
    switch (name) {
      case 'htmx:beforeCleanupElement':
        var internalData = api.getInternalData(evt.target)
        // Try to remove remove an EventSource when elements are removed
        if (internalData.sseEventSource) {
          internalData.sseEventSource.close()
        }
        return

      case 'htmx:afterProcessNode':
        return

      case 'htmx:trigger':
        ensureEventSourceOnElement(evt.target)
        triggerSSE(evt.target)

        return
    }
  },
})

///////////////////////////////////////////////
// HELPER FUNCTIONS
///////////////////////////////////////////////

/**
 * createEventSource is the default method for creating new EventSource objects.
 * it is hoisted into htmx.config.createEventSource to be overridden by the user, if needed.
 *
 * @param {string} url
 * @returns EventSource
 */
function createEventSource(url, options = {}) {
  return new SSE(url, { withCredentials: true, ...options })
}

function triggerSSE(elt) {
  // Set internalData and source
  var internalData = api.getInternalData(elt)
  var source = internalData.sseEventSource

  var sseEventNames = api
    .getAttributeValue(elt, 'hx-sse-events')
    ?.split(',')
    .map((s) => s.trim())

  sseEventNames.forEach((eventName) => {
    source.addEventListener(eventName, (event) => {
      var settleInfo = api.makeSettleInfo(elt)

      // api.oobSwap('true', eventElt, settleInfo)

      let eventElt = parseHTML(event.data)

      eventElt.childNodes.forEach((child) => {
        let oobValue = getAttributeValue(child, 'hx-swap-oob')
        if (oobValue) {
          api.oobSwap(oobValue, child, settleInfo)
        }
      })

      queryAttributeOnThisOrChildren(elt, 'sse-swap').forEach(function (child) {
        var sseSwapAttr = api.getAttributeValue(child, 'sse-swap')
        var shouldSwap = sseSwapAttr.split(',').find((n) => n === eventName)

        if (shouldSwap) {
          swap(child, event.data)
        }
      })

      api.triggerEvent(elt, 'htmx:sseMessage', { ...event, name: eventName })
    })
  })

  source.stream()
}

/**
 * ensureEventSourceOnElement creates a new EventSource connection on the provided element.
 * If a usable EventSource already exists, then it is returned.  If not, then a new EventSource
 * is created and stored in the element's internalData.
 * @param {HTMLElement} elt
 * @param {number} retryCount
 * @returns {EventSource | null}
 */
function ensureEventSourceOnElement(elt, retryCount) {
  if (elt == null) {
    return null
  }

  // HERE: Implement all verbs
  // handle extension source creation attribute
  queryAttributeOnThisOrChildren(elt, 'hx-sse-post').forEach(function (child) {
    var sseURL = api.getAttributeValue(child, 'hx-sse-post')
    if (sseURL == null) {
      return
    }

    ensureEventSource(child, sseURL, retryCount)
  })
}

function ensureEventSource(elt, url, retryCount) {
  const result = api.getInputValues(elt)
  const formData = makeFormData(result.values)
  const payload = new URLSearchParams(formData).toString()

  var source = htmx.createEventSource(url, {
    headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
    payload,
    method: 'POST',
    start: false,
  })

  source.onerror = function (err) {
    // Log an error event
    api.triggerErrorEvent(elt, 'htmx:sseError', {
      error: err,
      source: source,
    })

    if (source.readyState === EventSource.CLOSED) {
      retryCount = retryCount || 0
      var timeout = Math.random() * (2 ^ retryCount) * 500
      window.setTimeout(function () {
        ensureEventSourceOnElement(elt, Math.min(7, retryCount + 1))
      }, timeout)
    }
  }

  source.onopen = function (_evt) {
    api.triggerEvent(elt, 'htmx:sseOpen', { source: source })
  }

  api.getInternalData(elt).sseEventSource = source
}

/**
 * queryAttributeOnThisOrChildren returns all nodes that contain the requested attributeName, INCLUDING THE PROVIDED ROOT ELEMENT.
 *
 * @param {HTMLElement} elt
 * @param {string} attributeName
 */
function queryAttributeOnThisOrChildren(elt, attributeName) {
  var result = []

  // If the parent element also contains the requested attribute, then add it to the results too.
  if (api.hasAttribute(elt, attributeName)) {
    result.push(elt)
  }

  // Search all child nodes that match the requested attribute
  elt
    .querySelectorAll('[' + attributeName + '], [data-' + attributeName + ']')
    .forEach(function (node) {
      result.push(node)
    })

  return result
}

/**
 * @param {HTMLElement} elt
 * @param {string} content
 */
function swap(elt, content) {
  api.withExtensions(elt, function (extension) {
    content = extension.transformResponse(content, null, elt)
  })

  var swapSpec = api.getSwapSpecification(elt)
  var target = api.getTarget(elt)
  var settleInfo = api.makeSettleInfo(elt)

  api.selectAndSwap(swapSpec.swapStyle, target, elt, content, settleInfo)

  settleInfo.elts.forEach(function (elt) {
    if (elt.classList) {
      elt.classList.add(htmx.config.settlingClass)
    }
    api.triggerEvent(elt, 'htmx:beforeSettle')
  })

  // Handle settle tasks (with delay if requested)
  if (swapSpec.settleDelay > 0) {
    setTimeout(doSettle(settleInfo), swapSpec.settleDelay)
  } else {
    doSettle(settleInfo)()
  }
}

/*
 * HTMX INTERNALS COPY PASTA
 */

/**
 * @param {HTMLElement} elt
 * @param {string} name
 * @returns {(string | null)}
 */
function getRawAttribute(elt, name) {
  return elt.getAttribute && elt.getAttribute(name)
}

/**
 *
 * @param {HTMLElement} elt
 * @param {string} qualifiedName
 * @returns {(string | null)}
 */
function getAttributeValue(elt, qualifiedName) {
  return (
    getRawAttribute(elt, qualifiedName) ||
    getRawAttribute(elt, 'data-' + qualifiedName)
  )
}
/**
 * doSettle mirrors much of the functionality in htmx that
 * settles elements after their content has been swapped.
 * TODO: this should be published by htmx, and not duplicated here
 * @param {import("../htmx").HtmxSettleInfo} settleInfo
 * @returns () => void
 */
function doSettle(settleInfo) {
  return function () {
    settleInfo.tasks.forEach(function (task) {
      task.call()
    })

    settleInfo.elts.forEach(function (elt) {
      if (elt.classList) {
        elt.classList.remove(htmx.config.settlingClass)
      }
      api.triggerEvent(elt, 'htmx:afterSettle')
    })
  }
}

/**
 * @returns {Document}
 */
function getDocument() {
  return document
}

function parseHTML(resp, depth) {
  var parser = new DOMParser()
  var responseDoc = parser.parseFromString(resp, 'text/html')

  /** @type {Element} */
  var responseNode = responseDoc.body
  while (depth > 0) {
    depth--
    // @ts-ignore
    responseNode = responseNode.firstChild
  }
  if (responseNode == null) {
    // @ts-ignore
    responseNode = getDocument().createDocumentFragment()
  }
  return responseNode
}

function makeFormData(values) {
  var formData = new FormData()

  for (var name in values) {
    if (values.hasOwnProperty(name)) {
      var value = values[name]
      if (Array.isArray(value)) {
        forEach(value, function (v) {
          formData.append(name, v)
        })
      } else {
        formData.append(name, value)
      }
    }
  }
  return formData
}
