package dev.morpheus.app.ui

import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.defaultMinSize
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ArrowBack
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp

/** An unread count: the accent fill with text that reads on it (a muted room's is flat). */
@Composable
fun UnreadBadge(count: ULong, tag: String, muted: Boolean = false, modifier: Modifier = Modifier) {
    val fill = if (muted) Palette.Raised else Palette.Accent
    Pill(count.toString(), fill, if (muted) Palette.Muted else onColour(fill), tag, modifier)
}

/** A mention count: the danger fill, so it reads before an unread one. */
@Composable
fun MentionPill(count: ULong, tag: String, modifier: Modifier = Modifier) {
    Pill(count.toString(), Palette.Danger, Palette.OnDanger, tag, modifier)
}

@Composable
private fun Pill(text: String, fill: Color, content: Color, tag: String, modifier: Modifier) {
    Box(
        modifier.defaultMinSize(minWidth = 20.dp, minHeight = 20.dp).clip(CircleShape).background(fill).padding(horizontal = 6.dp),
        contentAlignment = Alignment.Center,
    ) {
        Text(text, Modifier.testTag(tag), color = content, style = Type.caption.copy(fontWeight = FontWeight.Bold))
    }
}


/** An icon that is a button: a 48 dp target, described for screen readers (and the tests, by `tag`). */
@Composable
fun IconAction(icon: ImageVector, description: String, tag: String, onClick: () -> Unit, modifier: Modifier = Modifier, tint: Color = Palette.Text) {
    Box(
        modifier
            .size(Size.target)
            .clip(CircleShape)
            .clickable(onClick = onClick)
            .semantics { contentDescription = description; role = Role.Button }
            .testTag(tag),
        contentAlignment = Alignment.Center,
    ) { Icon(icon, null, Modifier.size(24.dp), tint = tint) }
}

/** The bar on top of a screen: back, a title, actions; the colour of the rail, a hairline below. */
@Composable
fun TopBar(
    title: String,
    modifier: Modifier = Modifier,
    backTag: String? = "back",
    onBack: (() -> Unit)? = null,
    titleTag: String? = null,
    onTitle: (() -> Unit)? = null,
    actions: @Composable RowScope.() -> Unit = {},
) {
    Column(modifier.fillMaxWidth().background(Palette.Background)) {
        Row(
            Modifier.fillMaxWidth().heightIn(min = Size.topBar).padding(horizontal = Space.xs),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            if (onBack != null) IconAction(Icons.Default.ArrowBack, "Back", backTag ?: "back", onBack)
            else Box(Modifier.width(Space.md))
            Text(
                title,
                Modifier
                    .weight(1f)
                    .padding(horizontal = Space.xs)
                    .then(if (onTitle != null) Modifier.clickable(onClick = onTitle) else Modifier)
                    .then(if (titleTag != null) Modifier.testTag(titleTag) else Modifier),
                style = Type.heading,
                color = Palette.Text,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            actions()
        }
        Box(Modifier.fillMaxWidth().size(width = 0.dp, height = 1.dp).background(Palette.Divider))
    }
}

/** A small-caps heading over a group of rows. */
@Composable
fun SectionHeader(text: String, modifier: Modifier = Modifier) {
    Text(
        text.uppercase(),
        modifier.fillMaxWidth().padding(start = Space.lg, end = Space.lg, top = Space.lg, bottom = Space.xs),
        style = Type.label,
        color = Palette.Muted,
    )
}

/** Nothing here yet: an icon, what it is, and what to do about it. */
@Composable
fun EmptyState(icon: ImageVector, title: String, body: String, modifier: Modifier = Modifier, tag: String = "empty-state") {
    Column(
        modifier.fillMaxSize().padding(Space.xl).testTag(tag),
        verticalArrangement = Arrangement.Center,
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Box(Modifier.size(72.dp).clip(CircleShape).background(Palette.Raised), contentAlignment = Alignment.Center) {
            Icon(icon, null, Modifier.size(32.dp), tint = Palette.Muted)
        }
        Text(title, Modifier.padding(top = Space.lg), style = Type.heading, color = Palette.Text)
        Text(body, Modifier.padding(top = Space.xs), style = Type.secondary, color = Palette.Muted, textAlign = androidx.compose.ui.text.style.TextAlign.Center)
    }
}

/** A grey bar that breathes while the real thing loads. */
@Composable
fun SkeletonBar(width: androidx.compose.ui.unit.Dp, height: androidx.compose.ui.unit.Dp = 12.dp, round: Boolean = false) {
    val pulse by rememberInfiniteTransition(label = "skeleton").animateFloat(
        initialValue = 0.35f, targetValue = 0.75f,
        animationSpec = infiniteRepeatable(tween(900), RepeatMode.Reverse), label = "skeleton-alpha",
    )
    Box(
        Modifier.size(width, height).clip(if (round) CircleShape else RoundedCornerShape(Radius.sm))
            .background(Palette.Raised.copy(alpha = pulse)),
    )
}

/** A few rows' worth of skeleton, for a list that has not come yet. */
@Composable
fun SkeletonRows(count: Int = 6, avatars: Boolean = true, modifier: Modifier = Modifier) {
    Column(modifier.fillMaxWidth().testTag("skeleton"), verticalArrangement = Arrangement.spacedBy(Space.lg)) {
        repeat(count) { i ->
            Row(Modifier.padding(horizontal = Space.lg), horizontalArrangement = Arrangement.spacedBy(Space.md), verticalAlignment = Alignment.CenterVertically) {
                if (avatars) SkeletonBar(Size.avatar, Size.avatar, round = true)
                Column(verticalArrangement = Arrangement.spacedBy(Space.sm)) {
                    SkeletonBar((120 + (i * 37) % 90).dp)
                    SkeletonBar((180 + (i * 53) % 80).dp, 10.dp)
                }
            }
        }
    }
}

/** One of the desktop's icons (Lucide, generated into res/drawable by scripts/android-icons.sh), tinted. */
@Composable
fun GlyphIcon(@androidx.annotation.DrawableRes id: Int, modifier: Modifier = Modifier, size: androidx.compose.ui.unit.Dp = 24.dp, tint: Color = Palette.Text) {
    Icon(androidx.compose.ui.res.painterResource(id), null, modifier.size(size), tint = tint)
}
