package com.suppa.browser

import android.Manifest
import android.content.pm.PackageManager
import android.webkit.GeolocationPermissions
import android.webkit.PermissionRequest

/** A website asks for the camera, microphone or location: Suppa asks you first. */
object Perms {
    /** Sites you allowed until Suppa closes: "origin|what". */
    private val allowed = mutableSetOf<String>()

    fun webRequest(req: PermissionRequest) {
        val origin = req.origin.toString()
        val want = req.resources.filter {
            it == PermissionRequest.RESOURCE_VIDEO_CAPTURE || it == PermissionRequest.RESOURCE_AUDIO_CAPTURE ||
                it == PermissionRequest.RESOURCE_PROTECTED_MEDIA_ID
        }
        if (want.isEmpty()) {
            req.deny()
            return
        }
        // Protected media (DRM video) only: allowed without asking, like Chrome.
        if (want.all { it == PermissionRequest.RESOURCE_PROTECTED_MEDIA_ID }) {
            req.grant(want.toTypedArray())
            return
        }
        val names = want.mapNotNull {
            when (it) {
                PermissionRequest.RESOURCE_VIDEO_CAPTURE -> "camera"
                PermissionRequest.RESOURCE_AUDIO_CAPTURE -> "microphone"
                else -> null
            }
        }
        val key = origin + "|" + names.joinToString(",")
        val grant = {
            val android = mutableListOf<String>()
            if (PermissionRequest.RESOURCE_VIDEO_CAPTURE in want) android.add(Manifest.permission.CAMERA)
            if (PermissionRequest.RESOURCE_AUDIO_CAPTURE in want) android.add(Manifest.permission.RECORD_AUDIO)
            MainActivity.act.requestPerms(android.toTypedArray()) { ok ->
                if (ok) req.grant(want.toTypedArray()) else req.deny()
            }
        }
        if (key in allowed) {
            grant()
            return
        }
        Popup.dialog(
            "${U.siteOf(origin)} wants to use your ${names.joinToString(" and ")}",
            "Allow it for this visit? You can always say no.",
            listOf("Allow", "Block"), center = true,
        ) { i ->
            if (i == 0) {
                allowed.add(key)
                grant()
            } else {
                req.deny()
            }
        }
    }

    fun location(origin: String, cb: GeolocationPermissions.Callback) {
        Popup.dialog(
            "${U.siteOf(origin)} wants to know your location",
            "Allow it for this visit?",
            listOf("Allow", "Block"), center = true,
        ) { i ->
            if (i == 0) {
                MainActivity.act.requestPerms(arrayOf(Manifest.permission.ACCESS_FINE_LOCATION, Manifest.permission.ACCESS_COARSE_LOCATION)) { ok ->
                    cb.invoke(origin, ok, false)
                }
            } else {
                cb.invoke(origin, false, false)
            }
        }
    }

    fun has(p: String) = MainActivity.act.checkSelfPermission(p) == PackageManager.PERMISSION_GRANTED
}
