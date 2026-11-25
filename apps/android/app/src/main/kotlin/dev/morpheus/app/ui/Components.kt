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
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.wrapContentHeight
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LocalContentColor
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
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


/** An icon that is a button: a Material 3 IconButton (a 48 dp target with a ripple), described for screen readers (and the tests, by `tag`). */
@Composable
fun IconAction(@androidx.annotation.DrawableRes icon: Int, description: String, tag: String, onClick: () -> Unit, modifier: Modifier = Modifier, tint: Color = Color.Unspecified) {
    IconButton(
        onClick,
        modifier.size(Size.target).semantics { contentDescription = description }.testTag(tag),
    ) { GlyphIcon(icon, tint = tint) }
}

/** The bar on top of a screen: a Material 3 TopAppBar (back, a title, actions) in the colour of the rail, a hairline below. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun TopBar(
    title: String,
    modifier: Modifier = Modifier,
    backTag: String? = "back",
    onBack: (() -> Unit)? = null,
    titleTag: String? = null,
    onTitle: (() -> Unit)? = null,
    titleStyle: androidx.compose.ui.text.TextStyle? = null,
    container: Color = Color.Unspecified,
    actions: @Composable RowScope.() -> Unit = {},
) {
    val fill = if (container == Color.Unspecified) MaterialTheme.colorScheme.background else container
    Column(modifier.fillMaxWidth()) {
        // The bar is 56 dp tall here (a Material bar is 64): centred in a box that cuts the difference off, so every screen keeps its height.
        Box(Modifier.fillMaxWidth().height(Size.topBar).clipToBounds()) {
        TopAppBar(
            modifier = Modifier.wrapContentHeight(unbounded = true),
            title = {
                Text(
                    title,
                    Modifier
                        .then(if (onTitle != null) Modifier.clickable(onClick = onTitle) else Modifier)
                        .then(if (titleTag != null) Modifier.testTag(titleTag) else Modifier),
                    style = titleStyle ?: MaterialTheme.typography.titleMedium,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            },
            navigationIcon = {
                if (onBack != null) IconAction(dev.morpheus.app.R.drawable.ic_lucide_chevron_left, t("Back"), backTag ?: "back", onBack)
                else Box(Modifier.width(Space.lg))
            },
            actions = actions,
            windowInsets = WindowInsets(0),
            colors = TopAppBarDefaults.topAppBarColors(
                containerColor = fill,
                scrolledContainerColor = fill,
                titleContentColor = MaterialTheme.colorScheme.onBackground,
                navigationIconContentColor = MaterialTheme.colorScheme.onBackground,
                actionIconContentColor = MaterialTheme.colorScheme.onBackground,
            ),
        )
        }
        HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
    }
}

/** A Material 3 heading over a group inside a page: label-large in the primary colour (read against the page, #266). */
@Composable
fun SmallHeading(text: String, modifier: Modifier = Modifier) {
    Text(text, modifier, style = MaterialTheme.typography.labelLarge, color = readable(MaterialTheme.colorScheme.primary, MaterialTheme.colorScheme.surface))
}

/** A Material 3 section heading over a group of rows: label-large, in the primary colour. */
@Composable
fun SectionHeader(text: String, modifier: Modifier = Modifier) {
    Text(
        text,
        modifier.fillMaxWidth().padding(start = Space.lg, end = Space.lg, top = Space.lg, bottom = Space.xs),
        style = MaterialTheme.typography.labelLarge,
        color = readable(MaterialTheme.colorScheme.primary, MaterialTheme.colorScheme.surface),
    )
}

/** Nothing here yet: an icon, what it is, and what to do about it. */
@Composable
fun EmptyState(@androidx.annotation.DrawableRes icon: Int, title: String, body: String, modifier: Modifier = Modifier, tag: String = "empty-state") {
    Column(
        modifier.fillMaxSize().padding(Space.xl).testTag(tag),
        verticalArrangement = Arrangement.Center,
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Box(Modifier.size(72.dp).clip(CircleShape).background(Palette.Raised), contentAlignment = Alignment.Center) {
            GlyphIcon(icon, size = 32.dp, tint = Palette.Muted)
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
    androidx.compose.runtime.SideEffect { dev.morpheus.app.AppState.loadingSeen = true }
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
fun GlyphIcon(@androidx.annotation.DrawableRes id: Int, modifier: Modifier = Modifier, size: androidx.compose.ui.unit.Dp = 24.dp, tint: Color = Color.Unspecified) {
    Icon(androidx.compose.ui.res.painterResource(id), null, modifier.size(size), tint = if (tint == Color.Unspecified) LocalContentColor.current else tint)
}
