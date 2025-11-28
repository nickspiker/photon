package com.photon.messenger

import android.content.Context
import android.view.SurfaceView
import android.view.inputmethod.BaseInputConnection
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputConnection

/**
 * Custom SurfaceView that provides an InputConnection for receiving
 * IME text input (voice, swipe, autocomplete, etc.)
 */
class PhotonSurfaceView(
    context: Context,
    private val onTextInput: (String) -> Unit
) : SurfaceView(context) {

    init {
        isFocusable = true
        isFocusableInTouchMode = true
    }

    override fun onCreateInputConnection(outAttrs: EditorInfo): InputConnection {
        outAttrs.inputType = EditorInfo.TYPE_CLASS_TEXT
        outAttrs.imeOptions = EditorInfo.IME_FLAG_NO_FULLSCREEN or EditorInfo.IME_ACTION_DONE

        return object : BaseInputConnection(this, false) {
            override fun commitText(text: CharSequence?, newCursorPosition: Int): Boolean {
                text?.toString()?.let { onTextInput(it) }
                return true
            }

            override fun deleteSurroundingText(beforeLength: Int, afterLength: Int): Boolean {
                // Handle backspace from IME (some keyboards use this instead of key events)
                repeat(beforeLength) {
                    onTextInput("\b")  // We'll handle this as backspace in Rust
                }
                return true
            }
        }
    }
}
