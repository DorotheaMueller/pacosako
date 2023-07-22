module Header exposing (..)

{-| This module contains the page header.
-}

import Api.LocalStorage exposing (Permission(..))
import Api.Websocket
import Custom.Element exposing (icon)
import Element exposing (Element, alignRight, centerX, centerY, column, el, fill, height, padding, paddingEach, paddingXY, paragraph, px, row, spacing, width)
import Element.Background as Background
import Element.Border as Border
import Element.Font as Font
import Element.Input as Input
import FontAwesome.Solid as Solid
import FontAwesome.Styles
import Gen.Route as Route exposing (Route)
import Reactive
import Shared exposing (Msg(..))
import Svg.Custom
import Time
import Translations as T exposing (Language(..))


gamesArePublicHint : Shared.Model -> Element Shared.Msg
gamesArePublicHint model =
    if List.member HideGamesArePublicHint model.permissions then
        Element.none

    else
        Element.el [ width fill, Background.color (Element.rgb255 255 230 230), padding 10 ]
            (paragraph [ spacing 10 ]
                [ Element.text T.gamesArePublicHint
                , Input.button
                    [ Element.alignRight
                    , Background.color (Element.rgb255 255 200 200)
                    , Element.mouseOver [ Background.color (Element.rgb255 255 100 100) ]
                    , Border.rounded 5
                    , Element.padding 5
                    ]
                    { onPress = Just UserHidesGamesArePublicHint
                    , label = Element.text T.hideGamesArePublicHint
                    }
                ]
            )


type alias HeaderData =
    { isRouteHighlighted : Route -> Bool
    , isWithBackground : Bool
    }


{-| Header for the refactored ui.
-}
wrapWithHeaderV2 : Shared.Model -> (Shared.Msg -> msg) -> HeaderData -> Element msg -> Element msg
wrapWithHeaderV2 shared toMsg headerData body =
    Element.column
        [ width fill
        , height fill
        , Element.scrollbarY
        , if headerData.isWithBackground then
            Background.image "/a/bg.jpg"

          else
            Background.color (Element.rgb255 255 255 255)
        ]
        [ Element.html FontAwesome.Styles.css
        , pageHeaderV2 shared headerData
            |> Element.map toMsg
        , connectionIssueWarning shared
        , gamesArePublicHint shared
            |> Element.map toMsg
        , body
        ]


{-| Header that is shared by all pages.
-}
pageHeaderV2 : Shared.Model -> HeaderData -> Element Shared.Msg
pageHeaderV2 model headerData =
    case Reactive.classify model.windowSize of
        Reactive.Phone ->
            pageHeaderV2Phone model headerData model.isHeaderOpen

        Reactive.Tablet ->
            pageHeaderV2Phone model headerData model.isHeaderOpen

        Reactive.Desktop ->
            pageHeaderV2Desktop model headerData


pageHeaderV2Phone : Shared.Model -> HeaderData -> Bool -> Element Msg
pageHeaderV2Phone model headerData isHeaderOpen =
    column
        [ width fill
        , spacing 10
        , Background.color (Element.rgba255 255 255 255 0.6)
        ]
        [ row
            [ width fill
            , paddingEach { top = 10, bottom = 10, left = 15, right = 15 }
            , Border.solid
            , Border.widthEach
                { bottom = 1
                , left = 0
                , right = 0
                , top = 0
                }
            , Border.color (Element.rgb255 200 200 200)
            ]
            [ Input.button [ width (px 20) ]
                { onPress = Just (SetHeaderOpen (not isHeaderOpen))
                , label =
                    icon [ centerX, centerY, paddingXY 10 10 ]
                        (if isHeaderOpen then
                            Solid.times

                         else
                            Solid.bars
                        )
                }
            , el [ centerX ] pacosakoLogo
            , Input.button []
                { onPress = Just (SetHeaderOpen (not isHeaderOpen))
                , label = flagForLanguage T.compiledLanguage
                }
            ]
        , column
            [ spacing 10
            , paddingEach { top = 10, bottom = 10, left = 15, right = 15 }
            , width fill
            , Border.solid
            , Border.widthEach
                { bottom = 1
                , left = 0
                , right = 0
                , top = 0
                }
            , Border.color (Element.rgb255 200 200 200)
            ]
            [ pageHeaderButtonV2 Route.Home_ T.headerPlayPacoSako headerData.isRouteHighlighted
            , pageHeaderButtonV2 Route.Tutorial T.headerTutorial headerData.isRouteHighlighted
            , pageHeaderButtonV2 Route.Editor T.headerDesignPuzzles headerData.isRouteHighlighted
            , row [ centerX ] languageChoiceV2
            ]
            |> showIf isHeaderOpen
        ]


showIf : Bool -> Element msg -> Element msg
showIf condition element =
    if condition then
        element

    else
        Element.none


pageHeaderV2Desktop : Shared.Model -> HeaderData -> Element Msg
pageHeaderV2Desktop model headerData =
    Element.row
        [ width fill
        , Border.solid
        , Border.widthEach
            { bottom = 1
            , left = 0
            , right = 0
            , top = 0
            }
        , Border.color (Element.rgb255 200 200 200)
        , Background.color (Element.rgba255 255 255 255 0.6)
        ]
        [ Element.row
            [ width (Element.maximum 1120 fill)
            , centerX
            , Element.paddingXY 10 20
            , spacing 5
            ]
            [ Element.row [ spacing 15, width fill ]
                [ pageHeaderButtonV2 Route.Home_ T.headerPlayPacoSako headerData.isRouteHighlighted
                , pageHeaderButtonV2 Route.Tutorial T.headerTutorial headerData.isRouteHighlighted
                , pageHeaderButtonV2 Route.Editor T.headerDesignPuzzles headerData.isRouteHighlighted
                ]
            , el [] pacosakoLogo
            , el [ width fill ] (Element.row [ alignRight ] languageChoiceV2)
            ]
        ]


pageHeaderButtonV2 : Route -> String -> (Route -> Bool) -> Element Shared.Msg
pageHeaderButtonV2 route caption isRouteHighlighted =
    Element.link (pageHeaderStyle (isRouteHighlighted route))
        { url = Route.toHref route
        , label = Element.text caption
        }


pageHeaderStyle : Bool -> List (Element.Attribute msg)
pageHeaderStyle isRouteHighlighted =
    if isRouteHighlighted then
        [ Font.color (Element.rgb255 0 0 0)
        , Font.bold
        ]

    else
        [ Font.color (Element.rgb255 150 150 150)
        , Element.mouseOver [ Font.color (Element.rgb255 70 70 70) ]
        , Font.bold
        ]


pacosakoLogo : Element msg
pacosakoLogo =
    Element.image [ width (px 150), centerX ]
        { src = "/a/pacosako-logo.png", description = "PacoŜako logo" }


{-| Allows the user to choose the ui language.
-}
languageChoiceV2 : List (Element Shared.Msg)
languageChoiceV2 =
    [ Input.button [ padding 2 ]
        { onPress = Just (SetLanguage English)
        , label = Svg.Custom.flagEn |> Element.html
        }
    , Input.button [ padding 2 ]
        { onPress = Just (SetLanguage Dutch)
        , label = Svg.Custom.flagNl |> Element.html
        }
    , Input.button [ padding 2 ]
        { onPress = Just (SetLanguage Esperanto)
        , label = Svg.Custom.flagEo |> Element.html
        }
    , Input.button [ padding 2 ]
        { onPress = Just (SetLanguage Swedish)
        , label = Svg.Custom.flagSv |> Element.html
        }
    , Input.button [ padding 2 ]
        { onPress = Just (SetLanguage German)
        , label = Svg.Custom.flagDe |> Element.html
        }
    , Input.button [ padding 2 ]
        { onPress = Just (SetLanguage Spanish)
        , label = Svg.Custom.flagEs |> Element.html
        }
    ]


flagForLanguage : Language -> Element msg
flagForLanguage language =
    case language of
        English ->
            Svg.Custom.flagEn |> Element.html

        Dutch ->
            Svg.Custom.flagNl |> Element.html

        Esperanto ->
            Svg.Custom.flagEo |> Element.html

        Swedish ->
            Svg.Custom.flagSv |> Element.html

        German ->
            Svg.Custom.flagDe |> Element.html

        Spanish ->
            Svg.Custom.flagEs |> Element.html


connectionIssueWarning : Shared.Model -> Element msg
connectionIssueWarning model =
    case model.websocketConnectionState of
        Api.Websocket.WebsocketConnecting ->
            connectionIssueWithSeverity model T.websocketWarningConnecting

        Api.Websocket.WebsocketConnected ->
            Element.none

        Api.Websocket.WebsocketReconnecting ->
            connectionIssueWithSeverity model T.websocketWarningReconnecting


{-| Calculate how long the connection has been broken and wrap with
corresponding styles & second counter.
-}
connectionIssueWithSeverity : Shared.Model -> String -> Element msg
connectionIssueWithSeverity model text =
    let
        waitingTimeSeconds =
            (Time.posixToMillis model.now - Time.posixToMillis model.lastWebsocketStatusUpdate) // 1000
    in
    if waitingTimeSeconds < 2 then
        Element.none

    else if waitingTimeSeconds < 5 then
        Element.row
            [ width fill
            , Background.color (Element.rgb255 255 255 0)
            , Font.bold
            , paddingXY 10 10
            ]
            [ Element.text text ]

    else
        Element.row
            [ width fill
            , Background.color (Element.rgb255 255 0 0)
            , Font.bold
            , paddingXY 10 10
            ]
            [ Element.text (text ++ " (" ++ String.fromInt waitingTimeSeconds ++ "s)") ]
