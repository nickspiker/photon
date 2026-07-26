package com.photon.messenger

import android.content.Context
import android.view.SurfaceView
import android.view.inputmethod.BaseInputConnection
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputConnection
import android.view.inputmethod.InputMethodManager

/**
 * Custom SurfaceView providing an HONEST InputConnection: it mirrors the focused Rust textbox's
 * real text + cursor (fed per frame from nativeImeEditorText/Cursor) and answers surrounding-text
 * queries truthfully, and it expresses every IME mutation as a TRUE range replacement
 * (onImeReplace → nativeImeReplace) instead of the old backspace-replay-at-the-cursor hack.
 *
 * Why: Google voice typing reads the field back continuously (context, punctuation, verifying its
 * own commits) and rewrites EARLIER words via setComposingRegion. Against the old connection —
 * which claimed the field was always empty and could only edit at the cursor — the voice session
 * concluded the editor was corrupt and stopped mid-sentence.
 */
class PhotonSurfaceView(
    context: Context,
    private val onTextInput: (String) -> Unit,
    private val onImeReplace: (Int, Int, String) -> Unit = { _, _, s -> onTextInput(s) }
) : SurfaceView(context) {

    // The mirror of the focused Rust textbox: UTF-16 text + UTF-16 cursor + the composing span.
    // refreshed per frame by the Activity (refreshEditorMirror); the connection also applies its
    // own edits optimistically so reads between frames stay coherent.
    @Volatile var editorText: String = ""
    @Volatile var editorCursor: Int = 0
    private var composingStart = -1
    private var composingEnd = -1

    init {
        isFocusable = true
        isFocusableInTouchMode = true
    }

    /** Per-frame mirror refresh from Rust truth. On change, updateSelection tells the IME where the cursor really is — without it Gboard's internal model drifts and dictation aborts. */
    fun refreshEditorMirror(text: String, cursorUtf16: Int) {
        if (text == editorText && cursorUtf16 == editorCursor) return
        editorText = text
        editorCursor = cursorUtf16.coerceIn(0, text.length)
        if (composingStart > text.length || composingEnd > text.length) {
            composingStart = -1
            composingEnd = -1
        }
        val imm = context.getSystemService(Context.INPUT_METHOD_SERVICE) as InputMethodManager
        imm.updateSelection(this, editorCursor, editorCursor, composingStart, composingEnd)
    }

    /** UTF-16 offset → char (code point) count, for handing offsets to Rust (whose indices are chars). */
    private fun utf16ToChars(utf16: Int): Int =
        editorText.codePointCount(0, utf16.coerceIn(0, editorText.length))

    /** Apply a range replacement: optimistically to the local mirror (reads between frames stay truthful), and to the Rust textbox (in char offsets). */
    private fun replaceRange(startUtf16: Int, endUtf16: Int, text: String) {
        val s = startUtf16.coerceIn(0, editorText.length)
        val e = endUtf16.coerceIn(s, editorText.length)
        val cs = utf16ToChars(s)
        val ce = utf16ToChars(e)
        editorText = editorText.substring(0, s) + text + editorText.substring(e)
        editorCursor = s + text.length
        onImeReplace(cs, ce, text)
    }

    override fun onCreateInputConnection(outAttrs: EditorInfo): InputConnection {
        outAttrs.inputType = EditorInfo.TYPE_CLASS_TEXT
        outAttrs.imeOptions = EditorInfo.IME_FLAG_NO_FULLSCREEN or EditorInfo.IME_ACTION_DONE
        outAttrs.initialSelStart = editorCursor
        outAttrs.initialSelEnd = editorCursor
        composingStart = -1
        composingEnd = -1

        return object : BaseInputConnection(this, true) {

            override fun commitText(text: CharSequence?, newCursorPosition: Int): Boolean {
                val t = text?.toString() ?: return true
                if (composingStart >= 0) {
                    replaceRange(composingStart, composingEnd, t)
                    composingStart = -1
                    composingEnd = -1
                } else {
                    replaceRange(editorCursor, editorCursor, t)
                }
                reportSelection()
                return true
            }

            override fun setComposingText(text: CharSequence?, newCursorPosition: Int): Boolean {
                val t = text?.toString() ?: ""
                val start = if (composingStart >= 0) composingStart else editorCursor
                val end = if (composingStart >= 0) composingEnd else editorCursor
                replaceRange(start, end, t)
                if (t.isEmpty()) {
                    composingStart = -1
                    composingEnd = -1
                } else {
                    composingStart = start
                    composingEnd = start + t.length
                }
                reportSelection()
                return true
            }

            override fun setComposingRegion(start: Int, end: Int): Boolean {
                val s = start.coerceIn(0, editorText.length)
                val e = end.coerceIn(0, editorText.length)
                if (s == e) {
                    composingStart = -1
                    composingEnd = -1
                } else {
                    composingStart = minOf(s, e)
                    composingEnd = maxOf(s, e)
                }
                reportSelection()
                return true
            }

            override fun finishComposingText(): Boolean {
                composingStart = -1
                composingEnd = -1
                reportSelection()
                return true
            }

            override fun deleteSurroundingText(beforeLength: Int, afterLength: Int): Boolean {
                val s = (editorCursor - beforeLength).coerceAtLeast(0)
                val e = (editorCursor + afterLength).coerceAtMost(editorText.length)
                if (e > s) replaceRange(s, e, "")
                reportSelection()
                return true
            }

            override fun getTextBeforeCursor(n: Int, flags: Int): CharSequence {
                val c = editorCursor.coerceIn(0, editorText.length)
                return editorText.substring((c - n).coerceAtLeast(0), c)
            }

            override fun getTextAfterCursor(n: Int, flags: Int): CharSequence {
                val c = editorCursor.coerceIn(0, editorText.length)
                return editorText.substring(c, (c + n).coerceAtMost(editorText.length))
            }

            override fun getSelectedText(flags: Int): CharSequence? = null

            private fun reportSelection() {
                val imm = context.getSystemService(Context.INPUT_METHOD_SERVICE) as InputMethodManager
                imm.updateSelection(this@PhotonSurfaceView, editorCursor, editorCursor, composingStart, composingEnd)
            }
        }
    }
}
