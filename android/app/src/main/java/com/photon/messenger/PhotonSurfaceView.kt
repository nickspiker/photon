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
            // Track composing text so we can replace it when updated
            private var composingText = ""

            override fun commitText(text: CharSequence?, newCursorPosition: Int): Boolean {
                // Clear any composing text first (it's being replaced by commit)
                if (composingText.isNotEmpty()) {
                    repeat(composingText.length) { onTextInput("\b") }
                    composingText = ""
                }
                text?.toString()?.let { onTextInput(it) }
                return true
            }

            override fun setComposingText(text: CharSequence?, newCursorPosition: Int): Boolean {
                // Samsung/SwiftKey use composing text for predictive input
                // Delete old composing text, then insert new
                if (composingText.isNotEmpty()) {
                    repeat(composingText.length) { onTextInput("\b") }
                }
                composingText = text?.toString() ?: ""
                if (composingText.isNotEmpty()) {
                    onTextInput(composingText)
                }
                return true
            }

            override fun finishComposingText(): Boolean {
                // Composing text becomes permanent - just clear tracking
                composingText = ""
                return true
            }

            override fun deleteSurroundingText(beforeLength: Int, afterLength: Int): Boolean {
                // Handle backspace from IME (some keyboards use this instead of key events)
                // If we have composing text, adjust it
                if (composingText.isNotEmpty() && beforeLength > 0) {
                    val deleteCount = minOf(beforeLength, composingText.length)
                    repeat(deleteCount) { onTextInput("\b") }
                    composingText = composingText.dropLast(deleteCount)
                    // Any remaining deletes affect committed text
                    repeat(beforeLength - deleteCount) { onTextInput("\b") }
                } else {
                    repeat(beforeLength) {
                        onTextInput("\b")  // We'll handle this as backspace in Rust
                    }
                }
                return true
            }
        }
    }
}
