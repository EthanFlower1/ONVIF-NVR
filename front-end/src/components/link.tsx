/**
 * Link component adapted for React Router
 */

import * as Headless from '@headlessui/react'
import React, { forwardRef } from 'react'
import { Link as RouterLink } from 'react-router-dom'

export const Link = forwardRef(function Link(
  props: { href: string } & React.ComponentPropsWithoutRef<'a'>,
  ref: React.ForwardedRef<HTMLAnchorElement>
) {
  const { href, ...rest } = props
  
  // External links (starting with http:// or https://) should use a regular anchor tag
  if (href.startsWith('http://') || href.startsWith('https://')) {
    return (
      <Headless.DataInteractive>
        <a href={href} {...rest} ref={ref} />
      </Headless.DataInteractive>
    )
  }
  
  // Fragment links (#) should use a regular anchor tag
  if (href.startsWith('#')) {
    return (
      <Headless.DataInteractive>
        <a href={href} {...rest} ref={ref} />
      </Headless.DataInteractive>
    )
  }
  
  // Otherwise use React Router's Link component
  return (
    <Headless.DataInteractive>
      <RouterLink to={href} {...rest} ref={ref} />
    </Headless.DataInteractive>
  )
})